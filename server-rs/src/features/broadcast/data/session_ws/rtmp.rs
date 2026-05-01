use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use crate::core::contracts::workers::{SessionBundle, Stream};
use crate::features::broadcast::domain::output_health::OutputId;
use crate::features::broadcast::domain::render_graph::{
    RenderGraphId, RenderGraphNodeKind, RenderGraphOutputNode,
};
use crate::features::broadcast::domain::{Lang, LiveSession, SessionMetrics};

/// Wire value the frontend sends on a destination's `lang` field when the
/// user picks "Passthrough (source)" — the pipeline re-broadcasts host audio
/// unchanged and skips STT/translate/TTS for that stream. Kept in sync with
/// `@brivva/contracts/platforms` PASS_LANG_CODE.
pub const PASS_LANG_CODE: &str = "pass";

/// Pure URL transform for the FORCE_RTMP_NOT_RTMPS kill-switch. Exposed so
/// the kill-switch integration tests can exercise branch selection without
/// spawning FFmpeg.
pub fn maybe_downgrade_rtmps(url: &str, force_rtmp: bool) -> String {
    if force_rtmp && let Some(rest) = url.strip_prefix("rtmps://") {
        return format!("rtmp://{}", rest);
    }
    url.to_string()
}

/// Per-stream pipeline flags derived from the Workers-provided stream row +
/// the session's source language. Pulled out of `start_rtmp_streams` so the
/// decision ("passthrough? is_source? what gain?") can be unit-tested without
/// spawning FFmpeg.
#[derive(Debug, PartialEq)]
pub struct StreamPipelineFlags {
    pub is_source: bool,
    pub passthrough: bool,
    pub host_gain: f32,
}

pub fn resolve_stream_flags(
    stream_lang: &str,
    source_lang: &Lang,
    raw_gain: f32,
) -> StreamPipelineFlags {
    let passthrough = stream_lang == PASS_LANG_CODE;
    let is_source_match = Lang::from_str(stream_lang).is_some_and(|l| &l == source_lang);
    let is_source = passthrough || is_source_match;
    let host_gain = if passthrough {
        1.0
    } else {
        raw_gain.clamp(0.0, 1.0)
    };
    StreamPipelineFlags {
        is_source,
        passthrough,
        host_gain,
    }
}

pub fn output_id_for_stream(
    session_id: &str,
    stream: &Stream,
    seen: &mut HashMap<(String, String), usize>,
) -> Result<OutputId, crate::features::broadcast::domain::output_health::OutputHealthError> {
    let key = (
        stream.lang.trim().to_ascii_lowercase(),
        stream.platform.trim().to_ascii_lowercase(),
    );
    let index = seen.entry(key).and_modify(|n| *n += 1).or_insert(0);
    OutputId::new(session_id, &stream.lang, &stream.platform, *index)
}

pub(super) struct RtmpStartArgs<'a> {
    pub bundle: &'a SessionBundle,
    pub source_lang: &'a Lang,
    pub live_session: &'a mut LiveSession,
    pub sid: &'a str,
    pub ffmpeg_monitor_stop: Arc<AtomicBool>,
    pub metrics: Arc<SessionMetrics>,
}

pub(super) fn start_rtmp_streams(args: RtmpStartArgs<'_>) {
    let RtmpStartArgs {
        bundle,
        source_lang,
        live_session,
        sid,
        ffmpeg_monitor_stop,
        metrics,
    } = args;
    if bundle.streams.is_empty() {
        return;
    }
    let mut manager = crate::features::broadcast::data::ffmpeg::RtmpManager::new();
    manager.set_metrics(metrics);
    let mut rtmp_langs = Vec::new();
    let force_rtmp = live_session.pipeline_config.force_rtmp_not_rtmps;
    let output_controls_enabled = live_session.pipeline_config.v2_output_controls;
    let render_graph_enabled = live_session.pipeline_config.v2_render_graph;
    let render_graph_id = render_graph_enabled.then(|| RenderGraphId::for_session(sid));
    let mut output_indexes = HashMap::new();
    for s in &bundle.streams {
        let (Some(rtmp_url), Some(stream_key)) = (&s.rtmp_url, &s.stream_key) else {
            // Pre-flight guards this path; the only way to reach it now is a
            // bug upstream. Log loudly — the session has already been
            // accepted by this point and we don't want silent data-loss.
            tracing::error!(
                session_id = %sid,
                stream_id = %s.id,
                lang = %s.lang,
                "stream row passed preflight but has no rtmp_url/stream_key — pipeline bug"
            );
            continue;
        };
        let full_url_with_key = if stream_key.is_empty() {
            rtmp_url.clone()
        } else {
            format!("{}/{}", rtmp_url.trim_end_matches('/'), stream_key)
        };
        let full_url = maybe_downgrade_rtmps(&full_url_with_key, force_rtmp);
        if force_rtmp && full_url != full_url_with_key {
            tracing::warn!(
                stream_id = %s.id,
                "kill-switch FORCE_RTMP_NOT_RTMPS: downgraded rtmps:// to rtmp://"
            );
        }
        // User explicitly picked "Passthrough (source)" on this destination.
        // The pipeline MUST skip STT/translate/TTS: host audio RTMP'd raw at
        // full gain, no caption overlay. We implement it via the same
        // `is_source=true` switch target-lang streams already use, and carry
        // a dedicated `passthrough` flag so downstream code can distinguish
        // "user chose passthrough" from "stream's lang happens to equal
        // source_lang" for tracing / future bifurcation.
        let flags = resolve_stream_flags(&s.lang, source_lang, s.host_gain);
        let output_id = if output_controls_enabled || render_graph_enabled {
            match output_id_for_stream(sid, s, &mut output_indexes) {
                Ok(id) => Some(id),
                Err(e) => {
                    tracing::error!(
                        session_id = %sid,
                        stream_id = %s.id,
                        error = ?e,
                        "v2 output_id generation failed"
                    );
                    None
                }
            }
        } else {
            None
        };
        let render_graph_node = render_graph_id.as_ref().and_then(|graph_id| {
            output_id.as_ref().map(|output_id| {
                RenderGraphOutputNode::new(
                    graph_id.clone(),
                    output_id.as_str(),
                    &s.id,
                    RenderGraphNodeKind::EncodePublishRtmp,
                )
            })
        });
        let spawn_args = crate::features::broadcast::data::ffmpeg::StartStreamArgs {
            stream_id: &s.id,
            lang: &s.lang,
            rtmp_url: &full_url,
            delay_ms: s.delay_ms,
            is_source: flags.is_source,
            host_gain: flags.host_gain,
            output_id,
            destination_platform: &s.platform,
            output_controls_enabled,
            render_graph_node,
            passthrough: flags.passthrough,
        };
        if let Err(e) = manager.start_stream(spawn_args) {
            tracing::error!(
                stream_id = %s.id,
                lang = %s.lang,
                error = %e,
                "rtmp stream start failed"
            );
            continue;
        }
        // Passthrough streams don't participate in translation, so they must
        // not seed an STT/translate pipeline. Source-lang matches are pushed
        // like before — `dedupe_target_langs` filters them against the source.
        if !flags.passthrough
            && let Some(lang) = Lang::from_str(&s.lang)
        {
            rtmp_langs.push(lang);
        }
    }
    let shared_mgr = Arc::new(tokio::sync::Mutex::new(manager));
    live_session.rtmp_manager = Some(shared_mgr.clone());
    live_session.rtmp_langs = rtmp_langs;
    tracing::info!(
        session_id = %sid,
        stream_count = bundle.streams.len(),
        langs = ?live_session.rtmp_langs,
        "ffmpeg rtmp streams started"
    );
    let _health_monitor = crate::features::broadcast::data::ffmpeg::spawn_health_monitor(
        shared_mgr,
        ffmpeg_monitor_stop,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maybe_downgrade_rtmps_preserves_url_when_flag_is_off_even_for_rtmps() {
        let rewritten = maybe_downgrade_rtmps("rtmps://x/y", false);
        assert_eq!(rewritten, "rtmps://x/y");
    }

    #[test]
    fn maybe_downgrade_rtmps_does_not_rewrite_non_rtmps_schemes_even_with_flag() {
        assert_eq!(maybe_downgrade_rtmps("rtmp://x/y", true), "rtmp://x/y");
        assert_eq!(
            maybe_downgrade_rtmps("srt://host:9999?x=1", true),
            "srt://host:9999?x=1"
        );
    }

    fn stream(id: &str, lang: &str, platform: &str) -> Stream {
        Stream {
            id: id.into(),
            session_id: "sess-1".into(),
            lang: lang.into(),
            platform: platform.into(),
            platform_broadcast_id: None,
            platform_stream_id: None,
            stream_key: Some("k".into()),
            rtmp_url: Some("rtmp://x".into()),
            status: "ready".into(),
            delay_ms: 0,
            host_gain: 0.2,
            created_at: 0,
            watch_url: None,
        }
    }

    #[test]
    fn output_id_for_stream_uses_stable_per_lang_platform_index() {
        let mut seen = HashMap::new();
        let a = output_id_for_stream("B8EF28", &stream("a", "ja", "youtube"), &mut seen).unwrap();
        let b = output_id_for_stream("B8EF28", &stream("b", "ja", "youtube"), &mut seen).unwrap();
        let c = output_id_for_stream("B8EF28", &stream("c", "ja", "tiktok"), &mut seen).unwrap();
        let d = output_id_for_stream("B8EF28", &stream("d", "ko", "youtube"), &mut seen).unwrap();

        assert_eq!(a.as_str(), "B8EF28:ja:youtube:0");
        assert_eq!(b.as_str(), "B8EF28:ja:youtube:1");
        assert_eq!(c.as_str(), "B8EF28:ja:tiktok:0");
        assert_eq!(d.as_str(), "B8EF28:ko:youtube:0");
    }

    #[test]
    fn resolve_stream_flags_marks_pass_sentinel_as_passthrough_with_unit_gain() {
        let flags = resolve_stream_flags(PASS_LANG_CODE, &Lang::En, 0.2);
        assert!(flags.passthrough);
        assert!(
            flags.is_source,
            "passthrough must short-circuit drain via is_source"
        );
        assert!((flags.host_gain - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn resolve_stream_flags_pins_host_gain_at_unit_even_if_workers_sent_lower_value() {
        // Defensive: Workers sets host_gain to 1.0 when lang==source_lang, but
        // the "pass" sentinel is a separate wire value. If some migration
        // path slips a passthrough row through with host_gain=0.2 (the stale
        // target default), Fargate must still emit raw audio at full volume.
        let flags = resolve_stream_flags(PASS_LANG_CODE, &Lang::Ko, 0.2);
        assert!((flags.host_gain - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn resolve_stream_flags_preserves_existing_source_match_behavior_when_lang_equals_source() {
        // Historical path: user picked the source language as a target. We
        // keep treating it as is_source=true (no TTS, gain honored as-is).
        let flags = resolve_stream_flags("en", &Lang::En, 0.2);
        assert!(!flags.passthrough);
        assert!(flags.is_source);
        assert!((flags.host_gain - 0.2).abs() < f32::EPSILON);
    }

    #[test]
    fn resolve_stream_flags_translated_target_lang_is_not_source_and_clamps_gain() {
        let flags = resolve_stream_flags("ja", &Lang::En, 5.0);
        assert!(!flags.passthrough);
        assert!(!flags.is_source);
        assert!((flags.host_gain - 1.0).abs() < f32::EPSILON);

        let flags_neg = resolve_stream_flags("ja", &Lang::En, -0.5);
        assert!(flags_neg.host_gain.abs() < f32::EPSILON);
    }

    #[test]
    fn resolve_stream_flags_unknown_lang_code_treated_as_non_source_non_passthrough() {
        // Future-proofing: a new lang code lands in D1 before the server is
        // redeployed. Lang::from_str returns None, is_source=false, no
        // passthrough. The stream still starts (the FFmpeg layer accepts
        // arbitrary lang strings for caption/metrics labeling).
        let flags = resolve_stream_flags("th", &Lang::En, 0.2);
        assert!(!flags.passthrough);
        assert!(!flags.is_source);
    }
}

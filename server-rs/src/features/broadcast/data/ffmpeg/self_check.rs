use crate::features::broadcast::domain::VideoEncoderKind;
use std::process::Command;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeSelfCheckReport {
    pub ffmpeg_version_first_line: String,
    pub ffmpeg_config_has_openssl: bool,
    pub protocols_has_rtmp: bool,
    pub protocols_has_rtmps: bool,
    pub filters_has_drawtext: bool,
    pub encoders_has_h264_nvenc: bool,
    pub font_match: String,
    pub font_resolves_cjk: bool,
    pub required_nvenc: bool,
    pub ok: bool,
}

pub fn log_startup_runtime_self_check(video_encoder: VideoEncoderKind) {
    match run_runtime_self_check(video_encoder) {
        Ok(report) => log_report(&report),
        Err(error) => tracing::error!(
            check = "startup_runtime_self_check",
            error = %error,
            "RS007_RUNTIME_SELF_CHECK_FAIL startup_runtime_self_check command_failed"
        ),
    }
}

pub fn run_runtime_self_check(
    video_encoder: VideoEncoderKind,
) -> Result<RuntimeSelfCheckReport, String> {
    let version = run_capture("ffmpeg", &["-version"])?;
    let protocols = run_capture("ffmpeg", &["-hide_banner", "-protocols"])?;
    let filters = run_capture("ffmpeg", &["-hide_banner", "-filters"])?;
    let encoders = run_capture("ffmpeg", &["-hide_banner", "-encoders"])?;
    let font_match = run_capture("fc-match", &["Noto Sans CJK KR"])?;

    Ok(build_report(
        video_encoder,
        &version,
        &protocols,
        &filters,
        &encoders,
        &font_match,
    ))
}

pub fn build_report(
    video_encoder: VideoEncoderKind,
    version: &str,
    protocols: &str,
    filters: &str,
    encoders: &str,
    font_match: &str,
) -> RuntimeSelfCheckReport {
    let required_nvenc = video_encoder == VideoEncoderKind::Nvenc;
    let report = RuntimeSelfCheckReport {
        ffmpeg_version_first_line: version.lines().next().unwrap_or("").trim().to_string(),
        ffmpeg_config_has_openssl: has_token(version, "openssl"),
        protocols_has_rtmp: has_line_token(protocols, "rtmp"),
        protocols_has_rtmps: has_line_token(protocols, "rtmps"),
        filters_has_drawtext: has_line_token(filters, "drawtext"),
        encoders_has_h264_nvenc: has_line_token(encoders, "h264_nvenc"),
        font_match: font_match.lines().next().unwrap_or("").trim().to_string(),
        font_resolves_cjk: font_match.to_ascii_lowercase().contains("noto")
            && font_match.to_ascii_lowercase().contains("cjk"),
        required_nvenc,
        ok: false,
    };

    RuntimeSelfCheckReport {
        ok: report.ffmpeg_config_has_openssl
            && report.protocols_has_rtmp
            && report.protocols_has_rtmps
            && report.filters_has_drawtext
            && (!required_nvenc || report.encoders_has_h264_nvenc)
            && report.font_resolves_cjk,
        ..report
    }
}

fn log_report(report: &RuntimeSelfCheckReport) {
    let message = if report.ok {
        "RS007_RUNTIME_SELF_CHECK_OK startup_runtime_self_check complete"
    } else {
        "RS007_RUNTIME_SELF_CHECK_FAIL startup_runtime_self_check missing_required_capability"
    };

    tracing::info!(
        check = "startup_runtime_self_check",
        ok = report.ok,
        ffmpeg_version = %report.ffmpeg_version_first_line,
        ffmpeg_config_has_openssl = report.ffmpeg_config_has_openssl,
        protocols_has_rtmp = report.protocols_has_rtmp,
        protocols_has_rtmps = report.protocols_has_rtmps,
        filters_has_drawtext = report.filters_has_drawtext,
        encoders_has_h264_nvenc = report.encoders_has_h264_nvenc,
        required_nvenc = report.required_nvenc,
        font_match = %report.font_match,
        font_resolves_cjk = report.font_resolves_cjk,
        "{}",
        message
    );
}

fn run_capture(program: &str, args: &[&str]) -> Result<String, String> {
    let output = Command::new(program)
        .args(args)
        .output()
        .map_err(|error| format!("{program} spawn failed: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "{program} {:?} exited status={}",
            args, output.status
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn has_token(haystack: &str, token: &str) -> bool {
    haystack.to_ascii_lowercase().contains(token)
}

fn has_line_token(haystack: &str, token: &str) -> bool {
    haystack
        .lines()
        .flat_map(|line| line.split_whitespace())
        .any(|part| part.eq_ignore_ascii_case(token) || part.ends_with(token))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_passes_for_required_nvenc_capabilities() {
        let report = build_report(
            VideoEncoderKind::Nvenc,
            "ffmpeg version n6.1\nconfiguration: --enable-openssl --enable-libfreetype",
            "Input:\n  rtmp\n  rtmps",
            "Filters:\n T.C drawtext V->V Draw text",
            "Encoders:\n V..... h264_nvenc NVIDIA NVENC H.264 encoder",
            "NotoSansCJK-Regular.ttc: \"Noto Sans CJK KR\" \"Regular\"",
        );

        assert!(report.ok);
        assert!(report.required_nvenc);
    }

    #[test]
    fn report_fails_when_nvenc_requested_but_missing() {
        let report = build_report(
            VideoEncoderKind::Nvenc,
            "configuration: --enable-openssl",
            "rtmp\nrtmps",
            "drawtext",
            "libx264",
            "NotoSansCJK-Regular.ttc: \"Noto Sans CJK KR\" \"Regular\"",
        );

        assert!(!report.ok);
        assert!(!report.encoders_has_h264_nvenc);
    }

    #[test]
    fn x264_path_does_not_require_nvenc() {
        let report = build_report(
            VideoEncoderKind::X264,
            "configuration: --enable-openssl",
            "rtmp\nrtmps",
            "drawtext",
            "libx264",
            "NotoSansCJK-Regular.ttc: \"Noto Sans CJK KR\" \"Regular\"",
        );

        assert!(report.ok);
        assert!(!report.required_nvenc);
    }
}

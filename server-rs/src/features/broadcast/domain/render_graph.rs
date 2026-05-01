/// Stable identity for one localized output pipeline.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct OutputId(String);

impl OutputId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for OutputId {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

/// Source-video strategy for the render graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceVideoMode {
    EncodedPassthrough,
    DecodeNormalizeOnce,
}

/// Per-output render layer. Order matters: earlier layers feed later layers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderLayer {
    BurnedTranslatedSubtitles,
    TranslatedTtsAudio,
    LogoOverlay,
    ProductCtaText,
    LowerThird,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputPipelineSpec {
    pub output_id: OutputId,
    pub language_code: String,
    pub destination_label: String,
    pub layers: Vec<RenderLayer>,
    pub degradation_policy: DegradationPolicy,
}

impl OutputPipelineSpec {
    pub fn new(
        output_id: impl Into<OutputId>,
        language_code: impl Into<String>,
        destination_label: impl Into<String>,
        layers: Vec<RenderLayer>,
    ) -> Self {
        Self {
            output_id: output_id.into(),
            language_code: language_code.into(),
            destination_label: destination_label.into(),
            layers,
            degradation_policy: DegradationPolicy::default(),
        }
    }

    pub fn with_degradation_policy(mut self, degradation_policy: DegradationPolicy) -> Self {
        self.degradation_policy = degradation_policy;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderGraphSpec {
    pub source_video_mode: SourceVideoMode,
    pub outputs: Vec<OutputPipelineSpec>,
}

impl RenderGraphSpec {
    pub fn new(source_video_mode: SourceVideoMode, outputs: Vec<OutputPipelineSpec>) -> Self {
        Self {
            source_video_mode,
            outputs,
        }
    }

    pub fn output(&self, output_id: &OutputId) -> Option<&OutputPipelineSpec> {
        self.outputs
            .iter()
            .find(|output| &output.output_id == output_id)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputLifecycleState {
    Starting,
    Live,
    Degraded,
    Restarting,
    Failed,
    Stopped,
}

/// Stable identity for one in-process render graph adapter instance.
/// Phase 4 adapter-only: this names the current per-output FFmpeg path; it
/// does not imply shared decode, GPU workers, or a complex FFmpeg graph.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RenderGraphId(String);

impl RenderGraphId {
    pub fn for_session(session_id: impl AsRef<str>) -> Self {
        Self(format!("render-graph:{}", session_id.as_ref()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Stable identity for one output node inside the Phase 4 adapter graph.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RenderGraphNodeId(String);

impl RenderGraphNodeId {
    pub fn for_output(graph_id: &RenderGraphId, output_id: impl AsRef<str>) -> Self {
        Self(format!(
            "{}:output:{}",
            graph_id.as_str(),
            output_id.as_ref()
        ))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderGraphNodeKind {
    EncodePublishRtmp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderGraphNodeState {
    Starting,
    Live,
    Restarting,
    Failed,
    Stopped,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderGraphOutputNode {
    pub graph_id: RenderGraphId,
    pub node_id: RenderGraphNodeId,
    pub output_id: String,
    pub stream_id: String,
    pub kind: RenderGraphNodeKind,
    pub state: RenderGraphNodeState,
}

impl RenderGraphOutputNode {
    pub fn new(
        graph_id: RenderGraphId,
        output_id: impl Into<String>,
        stream_id: impl Into<String>,
        kind: RenderGraphNodeKind,
    ) -> Self {
        let output_id = output_id.into();
        let node_id = RenderGraphNodeId::for_output(&graph_id, &output_id);
        Self {
            graph_id,
            node_id,
            output_id,
            stream_id: stream_id.into(),
            kind,
            state: RenderGraphNodeState::Starting,
        }
    }

    pub fn transition(&mut self, state: RenderGraphNodeState) {
        self.state = state;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputDegradation {
    None,
    CaptionsOnly,
    DefaultVoice,
    OverlaysDisabled,
    Disabled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFailureKind {
    Tts,
    VoiceClone,
    SubtitleRender,
    OverlayRender,
    Encoder,
    RtmpDestination,
    RestartBudgetExhausted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DegradationAction {
    KeepLive,
    CaptionsOnly,
    UseDefaultVoice,
    DisableOverlays,
    RestartOutput,
    StopOutput,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DegradationPolicy {
    pub tts_failure: DegradationAction,
    pub voice_clone_failure: DegradationAction,
    pub subtitle_failure: DegradationAction,
    pub overlay_failure: DegradationAction,
    pub encoder_failure: DegradationAction,
    pub rtmp_failure: DegradationAction,
    pub restart_budget_exhausted: DegradationAction,
}

impl Default for DegradationPolicy {
    fn default() -> Self {
        Self {
            tts_failure: DegradationAction::CaptionsOnly,
            voice_clone_failure: DegradationAction::UseDefaultVoice,
            subtitle_failure: DegradationAction::KeepLive,
            overlay_failure: DegradationAction::DisableOverlays,
            encoder_failure: DegradationAction::RestartOutput,
            rtmp_failure: DegradationAction::RestartOutput,
            restart_budget_exhausted: DegradationAction::StopOutput,
        }
    }
}

impl DegradationPolicy {
    pub fn action_for(self, failure: OutputFailureKind) -> DegradationAction {
        match failure {
            OutputFailureKind::Tts => self.tts_failure,
            OutputFailureKind::VoiceClone => self.voice_clone_failure,
            OutputFailureKind::SubtitleRender => self.subtitle_failure,
            OutputFailureKind::OverlayRender => self.overlay_failure,
            OutputFailureKind::Encoder => self.encoder_failure,
            OutputFailureKind::RtmpDestination => self.rtmp_failure,
            OutputFailureKind::RestartBudgetExhausted => self.restart_budget_exhausted,
        }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct OutputQueueHealth {
    pub host_audio_chunks: usize,
    pub host_video_chunks: usize,
    pub tts_bytes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputHealth {
    pub output_id: OutputId,
    pub language_code: String,
    pub destination_label: String,
    pub state: OutputLifecycleState,
    pub degradation: OutputDegradation,
    pub restart_count: u32,
    pub last_write_age_ms: Option<u64>,
    pub queues: OutputQueueHealth,
    pub audio_drift_ms: Option<i64>,
    pub subtitle_lag_ms: Option<i64>,
}

impl OutputHealth {
    pub fn starting(
        output_id: impl Into<OutputId>,
        language_code: impl Into<String>,
        destination_label: impl Into<String>,
    ) -> Self {
        Self {
            output_id: output_id.into(),
            language_code: language_code.into(),
            destination_label: destination_label.into(),
            state: OutputLifecycleState::Starting,
            degradation: OutputDegradation::None,
            restart_count: 0,
            last_write_age_ms: None,
            queues: OutputQueueHealth::default(),
            audio_drift_ms: None,
            subtitle_lag_ms: None,
        }
    }

    pub fn apply_failure(&mut self, failure: OutputFailureKind, policy: DegradationPolicy) {
        match policy.action_for(failure) {
            DegradationAction::KeepLive => self.state = OutputLifecycleState::Live,
            DegradationAction::CaptionsOnly => {
                self.state = OutputLifecycleState::Degraded;
                self.degradation = OutputDegradation::CaptionsOnly;
            }
            DegradationAction::UseDefaultVoice => {
                self.state = OutputLifecycleState::Degraded;
                self.degradation = OutputDegradation::DefaultVoice;
            }
            DegradationAction::DisableOverlays => {
                self.state = OutputLifecycleState::Degraded;
                self.degradation = OutputDegradation::OverlaysDisabled;
            }
            DegradationAction::RestartOutput => {
                self.state = OutputLifecycleState::Restarting;
                self.restart_count = self.restart_count.saturating_add(1);
            }
            DegradationAction::StopOutput => {
                self.state = OutputLifecycleState::Stopped;
                self.degradation = OutputDegradation::Disabled;
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutputControlCommand {
    Restart(OutputId),
    Stop(OutputId),
    SetDegradation {
        output_id: OutputId,
        degradation: OutputDegradation,
    },
}

impl OutputControlCommand {
    pub fn output_id(&self) -> &OutputId {
        match self {
            Self::Restart(output_id) | Self::Stop(output_id) => output_id,
            Self::SetDegradation { output_id, .. } => output_id,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn live_health(id: &str) -> OutputHealth {
        let mut health = OutputHealth::starting(id, "ja", "YouTube");
        health.state = OutputLifecycleState::Live;
        health
    }

    #[test]
    fn render_graph_finds_output_by_stable_id() {
        let graph = RenderGraphSpec::new(
            SourceVideoMode::DecodeNormalizeOnce,
            vec![OutputPipelineSpec::new(
                "ja-youtube",
                "ja",
                "YouTube",
                vec![RenderLayer::BurnedTranslatedSubtitles],
            )],
        );

        assert_eq!(
            graph
                .output(&OutputId::from("ja-youtube"))
                .unwrap()
                .language_code,
            "ja"
        );
    }

    #[test]
    fn output_pipeline_spec_defaults_to_safe_degradation_policy() {
        let spec = OutputPipelineSpec::new("zh-grip", "zh", "Grip", vec![]);

        assert_eq!(
            spec.degradation_policy.action_for(OutputFailureKind::Tts),
            DegradationAction::CaptionsOnly
        );
    }

    #[test]
    fn render_graph_adapter_builds_stable_output_node_identity() {
        let graph_id = RenderGraphId::for_session("B8EF28");
        let node = RenderGraphOutputNode::new(
            graph_id.clone(),
            "B8EF28:ja:youtube:0",
            "stream-1",
            RenderGraphNodeKind::EncodePublishRtmp,
        );

        assert_eq!(graph_id.as_str(), "render-graph:B8EF28");
        assert_eq!(
            node.node_id.as_str(),
            "render-graph:B8EF28:output:B8EF28:ja:youtube:0"
        );
        assert_eq!(node.output_id, "B8EF28:ja:youtube:0");
        assert_eq!(node.stream_id, "stream-1");
        assert_eq!(node.kind, RenderGraphNodeKind::EncodePublishRtmp);
        assert_eq!(node.state, RenderGraphNodeState::Starting);
    }

    #[test]
    fn render_graph_adapter_tracks_lifecycle_state_transitions() {
        let mut node = RenderGraphOutputNode::new(
            RenderGraphId::for_session("B8EF28"),
            "B8EF28:ko:grip:0",
            "stream-2",
            RenderGraphNodeKind::EncodePublishRtmp,
        );

        node.transition(RenderGraphNodeState::Live);
        assert_eq!(node.state, RenderGraphNodeState::Live);
        node.transition(RenderGraphNodeState::Restarting);
        assert_eq!(node.state, RenderGraphNodeState::Restarting);
        node.transition(RenderGraphNodeState::Failed);
        assert_eq!(node.state, RenderGraphNodeState::Failed);
        node.transition(RenderGraphNodeState::Stopped);
        assert_eq!(node.state, RenderGraphNodeState::Stopped);
    }

    #[test]
    fn tts_failure_degrades_only_affected_output_to_captions() {
        let policy = DegradationPolicy::default();
        let mut ja = live_health("ja-youtube");
        let ko = live_health("ko-grip");

        ja.apply_failure(OutputFailureKind::Tts, policy);

        assert_eq!(ja.degradation, OutputDegradation::CaptionsOnly);
        assert_eq!(ko.degradation, OutputDegradation::None);
    }

    #[test]
    fn rtmp_failure_restarts_only_affected_output() {
        let policy = DegradationPolicy::default();
        let mut ja = live_health("ja-youtube");
        let zh = live_health("zh-grip");

        ja.apply_failure(OutputFailureKind::RtmpDestination, policy);

        assert_eq!(ja.state, OutputLifecycleState::Restarting);
        assert_eq!(ja.restart_count, 1);
        assert_eq!(zh.state, OutputLifecycleState::Live);
        assert_eq!(zh.restart_count, 0);
    }

    #[test]
    fn exhausted_restart_budget_stops_one_output_without_touching_others() {
        let policy = DegradationPolicy::default();
        let mut ja = live_health("ja-youtube");
        let zh = live_health("zh-grip");

        ja.apply_failure(OutputFailureKind::RestartBudgetExhausted, policy);

        assert_eq!(ja.state, OutputLifecycleState::Stopped);
        assert_eq!(ja.degradation, OutputDegradation::Disabled);
        assert_eq!(zh.state, OutputLifecycleState::Live);
    }

    #[test]
    fn output_control_command_exposes_target_output_id() {
        let command = OutputControlCommand::SetDegradation {
            output_id: OutputId::from("ja-youtube"),
            degradation: OutputDegradation::CaptionsOnly,
        };

        assert_eq!(command.output_id().as_str(), "ja-youtube");
    }
}

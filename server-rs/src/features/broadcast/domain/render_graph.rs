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

    pub fn for_shared_decode(graph_id: &RenderGraphId) -> Self {
        Self(format!("{}:shared-decode:source", graph_id.as_str()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderGraphNodeKind {
    SharedDecodeSource,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SharedDecodeNode {
    pub graph_id: RenderGraphId,
    pub node_id: RenderGraphNodeId,
    pub source_label: String,
    pub kind: RenderGraphNodeKind,
    pub state: RenderGraphNodeState,
}

impl SharedDecodeNode {
    pub fn new(graph_id: RenderGraphId, source_label: impl Into<String>) -> Self {
        let node_id = RenderGraphNodeId::for_shared_decode(&graph_id);
        Self {
            graph_id,
            node_id,
            source_label: source_label.into(),
            kind: RenderGraphNodeKind::SharedDecodeSource,
            state: RenderGraphNodeState::Starting,
        }
    }

    pub fn transition(&mut self, state: RenderGraphNodeState) {
        self.state = state;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SourceMediaKind {
    Video,
    Audio,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SourceMediaFrameId {
    pub source_node_id: String,
    pub media_kind: SourceMediaKind,
    pub pts_ms: u64,
    pub sequence: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceMediaFrame {
    pub id: SourceMediaFrameId,
    pub duration_ms: u32,
    pub payload_bytes: usize,
}

impl SourceMediaFrame {
    pub fn normalized(
        source: &SharedDecodeNode,
        media_kind: SourceMediaKind,
        pts_ms: u64,
        sequence: u64,
        duration_ms: u32,
        payload_bytes: usize,
    ) -> Self {
        Self {
            id: SourceMediaFrameId {
                source_node_id: source.node_id.as_str().to_string(),
                media_kind,
                pts_ms,
                sequence,
            },
            duration_ms,
            payload_bytes,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FanOutEdgeState {
    Healthy,
    Degraded,
    Stopped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackpressureAction {
    KeepLive,
    DropOldestFrames,
    MarkOutputDegraded,
    StopOutput,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FanOutDegradationReason {
    SlowConsumer,
    OutputFailed,
    RestartBudgetExhausted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BackpressurePolicy {
    pub max_queued_frames: usize,
    pub slow_consumer: BackpressureAction,
    pub output_failed: BackpressureAction,
    pub restart_budget_exhausted: BackpressureAction,
}

impl Default for BackpressurePolicy {
    fn default() -> Self {
        Self {
            max_queued_frames: 120,
            slow_consumer: BackpressureAction::MarkOutputDegraded,
            output_failed: BackpressureAction::MarkOutputDegraded,
            restart_budget_exhausted: BackpressureAction::StopOutput,
        }
    }
}

impl BackpressurePolicy {
    pub fn action_for(self, reason: FanOutDegradationReason) -> BackpressureAction {
        match reason {
            FanOutDegradationReason::SlowConsumer => self.slow_consumer,
            FanOutDegradationReason::OutputFailed => self.output_failed,
            FanOutDegradationReason::RestartBudgetExhausted => self.restart_budget_exhausted,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FanOutEdge {
    pub source_node_id: String,
    pub output_id: OutputId,
    pub queued_frames: usize,
    pub policy: BackpressurePolicy,
    pub state: FanOutEdgeState,
    pub degradation_reason: Option<FanOutDegradationReason>,
}

impl FanOutEdge {
    pub fn new(source: &SharedDecodeNode, output_id: impl Into<OutputId>) -> Self {
        Self {
            source_node_id: source.node_id.as_str().to_string(),
            output_id: output_id.into(),
            queued_frames: 0,
            policy: BackpressurePolicy::default(),
            state: FanOutEdgeState::Healthy,
            degradation_reason: None,
        }
    }

    pub fn with_policy(mut self, policy: BackpressurePolicy) -> Self {
        self.policy = policy;
        self
    }

    pub fn observe_queue_depth(&mut self, queued_frames: usize) {
        self.queued_frames = queued_frames;
        if queued_frames > self.policy.max_queued_frames {
            self.apply_degradation(FanOutDegradationReason::SlowConsumer);
        }
    }

    pub fn apply_degradation(&mut self, reason: FanOutDegradationReason) {
        match self.policy.action_for(reason) {
            BackpressureAction::KeepLive | BackpressureAction::DropOldestFrames => {
                self.state = FanOutEdgeState::Healthy;
                self.degradation_reason = None;
            }
            BackpressureAction::MarkOutputDegraded => {
                self.state = FanOutEdgeState::Degraded;
                self.degradation_reason = Some(reason);
            }
            BackpressureAction::StopOutput => {
                self.state = FanOutEdgeState::Stopped;
                self.degradation_reason = Some(reason);
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputEncodeNode {
    pub graph_id: RenderGraphId,
    pub node_id: RenderGraphNodeId,
    pub output_id: OutputId,
    pub stream_id: String,
    pub state: RenderGraphNodeState,
    pub restart_count: u32,
}

impl OutputEncodeNode {
    pub fn new(
        graph_id: RenderGraphId,
        output_id: impl Into<OutputId>,
        stream_id: impl Into<String>,
    ) -> Self {
        let output_id = output_id.into();
        let node_id = RenderGraphNodeId::for_output(&graph_id, output_id.as_str());
        Self {
            graph_id,
            node_id,
            output_id,
            stream_id: stream_id.into(),
            state: RenderGraphNodeState::Starting,
            restart_count: 0,
        }
    }

    pub fn apply_restart_budget_exhausted(&mut self) {
        self.state = RenderGraphNodeState::Failed;
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

    #[test]
    fn shared_decode_node_builds_stable_source_identity_and_normalized_frames() {
        let source = SharedDecodeNode::new(RenderGraphId::for_session("B8EF28"), "host-webrtc");
        let video =
            SourceMediaFrame::normalized(&source, SourceMediaKind::Video, 1_000, 42, 33, 4096);
        let audio =
            SourceMediaFrame::normalized(&source, SourceMediaKind::Audio, 1_020, 43, 20, 1764);

        assert_eq!(source.kind, RenderGraphNodeKind::SharedDecodeSource);
        assert_eq!(
            source.node_id.as_str(),
            "render-graph:B8EF28:shared-decode:source"
        );
        assert_eq!(video.id.source_node_id, source.node_id.as_str());
        assert_eq!(video.id.media_kind, SourceMediaKind::Video);
        assert_eq!(video.id.pts_ms, 1_000);
        assert_eq!(video.id.sequence, 42);
        assert_eq!(audio.id.media_kind, SourceMediaKind::Audio);
        assert_eq!(audio.duration_ms, 20);
    }

    #[test]
    fn one_shared_source_fans_out_to_many_output_encode_nodes() {
        let graph_id = RenderGraphId::for_session("B8EF28");
        let source = SharedDecodeNode::new(graph_id.clone(), "host-webrtc");
        let ja = OutputEncodeNode::new(graph_id.clone(), "B8EF28:ja:youtube:0", "stream-ja");
        let zh = OutputEncodeNode::new(graph_id, "B8EF28:zh:grip:0", "stream-zh");
        let ja_edge = FanOutEdge::new(&source, ja.output_id.clone());
        let zh_edge = FanOutEdge::new(&source, zh.output_id.clone());

        assert_eq!(ja_edge.source_node_id, source.node_id.as_str());
        assert_eq!(zh_edge.source_node_id, source.node_id.as_str());
        assert_ne!(ja.output_id, zh.output_id);
        assert_eq!(
            ja.node_id.as_str(),
            "render-graph:B8EF28:output:B8EF28:ja:youtube:0"
        );
        assert_eq!(
            zh.node_id.as_str(),
            "render-graph:B8EF28:output:B8EF28:zh:grip:0"
        );
    }

    #[test]
    fn slow_output_degrades_only_its_fanout_edge() {
        let source = SharedDecodeNode::new(RenderGraphId::for_session("B8EF28"), "host-webrtc");
        let mut slow =
            FanOutEdge::new(&source, "B8EF28:ja:youtube:0").with_policy(BackpressurePolicy {
                max_queued_frames: 2,
                ..BackpressurePolicy::default()
            });
        let healthy = FanOutEdge::new(&source, "B8EF28:ko:grip:0");

        slow.observe_queue_depth(3);

        assert_eq!(slow.state, FanOutEdgeState::Degraded);
        assert_eq!(
            slow.degradation_reason,
            Some(FanOutDegradationReason::SlowConsumer)
        );
        assert_eq!(healthy.state, FanOutEdgeState::Healthy);
        assert_eq!(healthy.degradation_reason, None);
    }

    #[test]
    fn restart_budget_exhaustion_affects_output_not_source_decode() {
        let graph_id = RenderGraphId::for_session("B8EF28");
        let source = SharedDecodeNode::new(graph_id.clone(), "host-webrtc");
        let mut output = OutputEncodeNode::new(graph_id, "B8EF28:ja:youtube:0", "stream-ja");
        let mut edge = FanOutEdge::new(&source, output.output_id.clone());

        output.apply_restart_budget_exhausted();
        edge.apply_degradation(FanOutDegradationReason::RestartBudgetExhausted);

        assert_eq!(output.state, RenderGraphNodeState::Failed);
        assert_eq!(edge.state, FanOutEdgeState::Stopped);
        assert_eq!(source.state, RenderGraphNodeState::Starting);
        assert_eq!(
            source.node_id.as_str(),
            "render-graph:B8EF28:shared-decode:source"
        );
    }
}

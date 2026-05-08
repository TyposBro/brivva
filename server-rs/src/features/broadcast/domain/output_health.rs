use serde::{Deserialize, Serialize};

/// Stable V2 output identity. RFC format:
/// `{session_id}:{lang}:{destination_platform}:{index}`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct OutputId(String);

impl OutputId {
    pub fn new(
        session_id: impl AsRef<str>,
        lang: impl AsRef<str>,
        destination_platform: impl AsRef<str>,
        index: usize,
    ) -> Result<Self, OutputHealthError> {
        let session_id = clean_part("session_id", session_id.as_ref())?;
        let lang = clean_part("lang", lang.as_ref())?;
        let destination_platform =
            clean_part("destination_platform", destination_platform.as_ref())?;
        Ok(Self(format!(
            "{session_id}:{lang}:{destination_platform}:{index}"
        )))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for OutputId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

fn clean_part(label: &'static str, value: &str) -> Result<String, OutputHealthError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(OutputHealthError::EmptyIdentityPart { label });
    }
    if trimmed.contains(':') {
        return Err(OutputHealthError::InvalidIdentityPart { label });
    }
    Ok(trimmed.to_string())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputHealthState {
    Starting,
    /// FFmpeg is successfully publishing bytes to the RTMP endpoint. This is
    /// internal output health, not provider-confirmed public live status.
    Publishing,
    Live,
    Degraded,
    Restarting,
    Failed,
    Stopped,
}

impl OutputHealthState {
    pub fn event_name(self) -> &'static str {
        match self {
            Self::Starting => "output.starting",
            Self::Publishing => "output.publishing",
            Self::Live => "output.live",
            Self::Degraded => "output.degraded",
            Self::Restarting => "output.restarting",
            Self::Failed => "output.failed",
            Self::Stopped => "output.stopped",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputDegradationLabel {
    SlowEncode,
    DroppedFrames,
    IdleNoWrites,
    FfmpegCrash,
    RestartLimitReached,
    RtmpPublishError,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OutputHealthSnapshot {
    pub output_id: OutputId,
    pub stream_id: String,
    pub lang: String,
    pub destination_platform: String,
    pub state: OutputHealthState,
    pub restart_count: u32,
    pub last_write_ms: Option<i64>,
    pub degradation: Option<OutputDegradationLabel>,
    pub message: Option<String>,
}

impl OutputHealthSnapshot {
    pub fn event_name(&self) -> &'static str {
        self.state.event_name()
    }

    pub fn new(
        output_id: OutputId,
        stream_id: impl Into<String>,
        lang: impl Into<String>,
        destination_platform: impl Into<String>,
        state: OutputHealthState,
    ) -> Self {
        Self {
            output_id,
            stream_id: stream_id.into(),
            lang: lang.into(),
            destination_platform: destination_platform.into(),
            state,
            restart_count: 0,
            last_write_ms: None,
            degradation: None,
            message: None,
        }
    }

    pub fn with_restart_count(mut self, restart_count: u32) -> Self {
        self.restart_count = restart_count;
        self
    }

    pub fn with_last_write_ms(mut self, last_write_ms: i64) -> Self {
        self.last_write_ms = Some(last_write_ms);
        self
    }

    pub fn degraded(mut self, label: OutputDegradationLabel, message: impl Into<String>) -> Self {
        self.state = OutputHealthState::Degraded;
        self.degradation = Some(label);
        self.message = Some(message.into());
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "command", rename_all = "snake_case")]
pub enum OutputControlCommand {
    RestartOutput { output_id: OutputId },
    StopOutput { output_id: OutputId },
    SetCaptionsOnly { output_id: OutputId, enabled: bool },
    SetDefaultVoice { output_id: OutputId, enabled: bool },
    SetNoOverlays { output_id: OutputId, enabled: bool },
}

impl OutputControlCommand {
    pub fn output_id(&self) -> &OutputId {
        match self {
            Self::RestartOutput { output_id }
            | Self::StopOutput { output_id }
            | Self::SetCaptionsOnly { output_id, .. }
            | Self::SetDefaultVoice { output_id, .. }
            | Self::SetNoOverlays { output_id, .. } => output_id,
        }
    }

    /// Phase 3 contract proof only: commands are validated/typed, not executed.
    pub fn validate(&self) -> Result<(), OutputHealthError> {
        if self.output_id().as_str().split(':').count() != 4 {
            return Err(OutputHealthError::MalformedOutputId);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutputHealthError {
    EmptyIdentityPart { label: &'static str },
    InvalidIdentityPart { label: &'static str },
    MalformedOutputId,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn output_id() -> OutputId {
        OutputId::new("B8EF28", "JA", "YouTube", 0).unwrap()
    }

    #[test]
    fn output_id_uses_rfc_format_and_trims_identity_parts() {
        assert_eq!(output_id().as_str(), "B8EF28:JA:YouTube:0");
        assert_eq!(
            OutputId::new(" B8EF28 ", " ko ", " TikTok ", 3)
                .unwrap()
                .to_string(),
            "B8EF28:ko:TikTok:3"
        );
    }

    #[test]
    fn output_id_rejects_empty_or_colon_parts() {
        assert_eq!(
            OutputId::new("", "ja", "youtube", 0),
            Err(OutputHealthError::EmptyIdentityPart {
                label: "session_id"
            })
        );
        assert_eq!(
            OutputId::new("s", "ja:bad", "youtube", 0),
            Err(OutputHealthError::InvalidIdentityPart { label: "lang" })
        );
    }

    #[test]
    fn health_states_map_to_logs_first_event_names() {
        assert_eq!(OutputHealthState::Starting.event_name(), "output.starting");
        assert_eq!(
            OutputHealthState::Publishing.event_name(),
            "output.publishing"
        );
        assert_eq!(OutputHealthState::Live.event_name(), "output.live");
        assert_eq!(OutputHealthState::Degraded.event_name(), "output.degraded");
        assert_eq!(
            OutputHealthState::Restarting.event_name(),
            "output.restarting"
        );
        assert_eq!(OutputHealthState::Failed.event_name(), "output.failed");
        assert_eq!(OutputHealthState::Stopped.event_name(), "output.stopped");
    }

    #[test]
    fn health_snapshot_tracks_state_transitions_and_degradation_label() {
        let snapshot = OutputHealthSnapshot::new(
            output_id(),
            "stream-1",
            "ja",
            "youtube",
            OutputHealthState::Starting,
        )
        .with_restart_count(2)
        .with_last_write_ms(123)
        .degraded(OutputDegradationLabel::SlowEncode, "speed below realtime");

        assert_eq!(snapshot.state, OutputHealthState::Degraded);
        assert_eq!(snapshot.restart_count, 2);
        assert_eq!(snapshot.last_write_ms, Some(123));
        assert_eq!(
            snapshot.degradation,
            Some(OutputDegradationLabel::SlowEncode)
        );
        assert_eq!(snapshot.message.as_deref(), Some("speed below realtime"));
    }

    #[test]
    fn control_command_vocabulary_validates_known_commands_without_execution() {
        let id = output_id();
        let commands = [
            OutputControlCommand::RestartOutput {
                output_id: id.clone(),
            },
            OutputControlCommand::StopOutput {
                output_id: id.clone(),
            },
            OutputControlCommand::SetCaptionsOnly {
                output_id: id.clone(),
                enabled: true,
            },
            OutputControlCommand::SetDefaultVoice {
                output_id: id.clone(),
                enabled: true,
            },
            OutputControlCommand::SetNoOverlays {
                output_id: id,
                enabled: true,
            },
        ];

        for command in commands {
            assert!(command.validate().is_ok(), "{command:?}");
        }
    }
}

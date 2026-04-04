//! Gladia response types for STT messages.

// ── Gladia Response Types ────────────────────────────────

#[derive(Debug, serde::Deserialize)]
pub struct GladiaMessage {
    #[serde(rename = "type", default)]
    pub msg_type: String,
    #[serde(default)]
    pub data: Option<GladiaData>,
    #[serde(default)]
    pub error: Option<GladiaError>,
}

#[derive(Debug, serde::Deserialize)]
pub struct GladiaData {
    #[serde(default)]
    pub is_final: bool,
    #[serde(default)]
    pub utterance: Option<GladiaUtterance>,
}

#[derive(Debug, serde::Deserialize)]
pub struct GladiaUtterance {
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub language: String,
    #[serde(default)]
    pub confidence: f64,
}

#[derive(Debug, serde::Deserialize)]
pub struct GladiaError {
    #[serde(default)]
    pub message: String,
    #[serde(default)]
    pub status_code: u16,
}

#[derive(Debug, serde::Deserialize)]
pub struct GladiaSession {
    pub id: String,
    pub url: String,
}

impl GladiaMessage {
    /// Extract the transcript text from a transcript message.
    pub fn transcript(&self) -> Option<String> {
        self.data
            .as_ref()
            .and_then(|d| d.utterance.as_ref())
            .map(|u| u.text.trim().to_string())
            .filter(|t| !t.is_empty())
    }

    pub fn is_final(&self) -> bool {
        self.data.as_ref().map(|d| d.is_final).unwrap_or(false)
    }
}

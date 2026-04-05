use std::time::Instant;

/// A sub-utterance chunk ready for translation + TTS.
#[allow(dead_code)]
pub struct ChunkEvent {
    pub text: String,
    pub chunk_index: u16,
    pub context: Option<String>,
    pub is_utterance_final: bool,
    pub utterance_id: u64,
    pub utterance_start: Instant,
    pub host_audio: Vec<u8>,
}

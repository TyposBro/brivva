// WebSocket binary message type tags
pub const MSG_TAG_AUDIO: u8 = 0x01;
pub const MSG_TAG_VIDEO: u8 = 0x02;

// Audio format
pub const SAMPLE_RATE: u32 = 44100;
pub const CHANNELS: u16 = 1;
pub const BITS_PER_SAMPLE: u16 = 16;
pub const BYTES_PER_SEC: f64 = 88200.0; // SAMPLE_RATE * (BITS_PER_SAMPLE/8) * CHANNELS

// Server
pub const SERVER_ADDR: &str = "127.0.0.1:3000";
pub const MAX_BODY_SIZE: usize = 20 * 1024 * 1024; // 20MB (3min @ 44100Hz 16-bit mono = ~15.1MB)

// Defaults
pub const DEFAULT_BROADCAST_DELAY_MS: u64 = 3000;
pub const DEFAULT_TTS_MODEL: &str = "eleven_turbo_v2_5";
pub const DEFAULT_VOICE_ID: &str = "21m00Tcm4TlvDq8ikWAM";
pub const VOICE_CLONE_FILE: &str = ".brivva_voice_clone";
pub const VOICE_CLONE_FILE_DASHSCOPE: &str = ".brivva_voice_clone_dashscope";

// DashScope / Qwen3-TTS
/// Voice-clone realtime model — requires enrolled voice ID.
pub const DASHSCOPE_TTS_MODEL_VC: &str = "qwen3-tts-vc-realtime-2026-01-15";
/// Flash realtime model — supports built-in preset voices (used when no clone enrolled).
pub const DASHSCOPE_TTS_MODEL_FLASH: &str = "qwen3-tts-flash-realtime";
pub const DEFAULT_TTS_PROVIDER: &str = "elevenlabs";

/// Per-language default voice for ElevenLabs (built-in/library voices).
/// Returns (female_voice_id, male_voice_id).
pub fn elevenlabs_default_voices(lang: &str) -> (&'static str, &'static str) {
    match lang {
        "zh" => ("9lHjugDhwqoxA5MhX0az", "brChkoggsUHF1stW6omH"),
        "ja" => ("xwDy9oDEtzWzFo6FqAI9", "LIisRj2veIKEBdr6KZ5y"),
        "ko" => ("zgDzx5jLLCqEp6Fl7Kl7", "m3gJBS8OofDJfycyA2Ip"), // Jessica (ko), Eric (ko)
        "ru" => ("gelrownZgbRhxH6LI78J", "85bJFRap3VIXOThFHxk3"),
        _    => ("4CrZuIW9am7gYAxgo2Af", "JdwJ7jL68CWmQZuo7KgG"),
    }
}

/// Per-language default voice for DashScope/Qwen3-TTS (preset voices).
pub fn dashscope_default_voices(lang: &str) -> (&'static str, &'static str) {
    match lang {
        "zh" => ("Cherry", "Ethan"),
        // Qwen3 presets are Chinese-native; use same for other langs until better options exist.
        _    => ("Cherry", "Ethan"),
    }
}

// TTS timing
pub const TTS_DEADLINE_CAP_MS: u64 = 10_000;
pub const TTS_DEADLINE_MARGIN_MS: u64 = 500;
pub const TTS_DEADLINE_FLOOR_MS: u64 = 3_000;

// Pipeline
pub const STREAMING_BUDGET_PADDING_SECS: f64 = 5.0;
pub const STT_RECONNECT_MAX: u32 = 5;
pub const STT_RECONNECT_DELAY_SECS: u64 = 1;

// Soniox
pub const SONIOX_MAX_ENDPOINT_DELAY_MS: u64 = 1500;

// Recording & dubbing
pub const RECORDING_DIR: &str = "/tmp/brivva/recordings";
pub const DUBBING_DIR: &str = "/tmp/brivva/dubbing";

// Tier
pub const TIER_SUBTITLES_ONLY: u8 = 1;

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

// DashScope / Qwen3-TTS
pub const DASHSCOPE_TTS_MODEL_VC: &str = "qwen3-tts-vc-realtime-2026-01-15";
pub const DEFAULT_TTS_PROVIDER: &str = "elevenlabs";

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

// Tier
pub const TIER_SUBTITLES_ONLY: u8 = 1;

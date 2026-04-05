// --- constants ---

export const MAX_GAP_MS = 1500;
export const TARGET_MS = 300;
// TODO: verify usage
export const TOTAL_CURRENT_MS = 862;
export const TOTAL_GAP_MULTIPLIER = 2.9;

// --- types ---

export type Row = string[];

export type GapItem = {
  label: string;
  currentMs: number;
  targetMs: number;
  note?: string;
};

// --- pipeline comparisons ---

export const MY_PIPELINE: Row[] = [
  ["STT", "CF Nova-3 (streaming via stt-wrapper)", "streaming"],
  ["Translation", "NLLB-200-distilled-600M (self-hosted)", "170ms avg"],
  ["TTS", "ElevenLabs eleven_flash_v2_5", "539ms avg"],
  ["Lip-sync", "Wav2Lip + GFPGAN (self-hosted GPU)", "built, testing"],
  ["Voice Clone", "ElevenLabs IVC (5s PCM)", "not working"],
  ["Emotion TTS", "Prosody extraction + style mapping", "not working"],
  ["Server", "Rust axum + DashMap + fan-out/lang", "orchestration"],
];

export const BRIVVA_PIPELINE: Row[] = [
  ["STT", "Whisper", "batch only"],
  ["Translation", "Context NMT", "unknown"],
  ["TTS", "Emotive TTS (3B)", "unknown"],
  ["Lip-sync", "Wav2Lip", "lips only"],
  ["Target", "—", "<300ms e2e"],
];

// --- component decision tables ---

export const STT_ROWS: Row[] = [
  ["✓ CF Nova-3 (stt-wrapper)", "6.5%", "Yes — streaming", "CF AI Gateway"],
  ["WhisperLiveKit (base.en)", "—", "Yes — live interims", "Free (self-hosted)"],
  ["Whisper v3 Turbo", "4.8%", "No — batch only", "$0.67/1000min"],
];

export const TRANSLATION_ROWS: Row[] = [
  ["✓ NLLB-200-distilled-600M (GPU)", "Seq2Seq", "82–164ms"],
  ["M2M100-1.2B (CF Workers AI)", "Seq2Seq", "394–870ms"],
  ["Llama 3.2 1B", "LLM prompted", "~1,500ms"],
];

export const TTS_ROWS: Row[] = [
  ["✓ ElevenLabs eleven_flash_v2_5", "API", "548–1,440ms", "Yes — 32 langs"],
  ["Kokoro 82M (self-hosted)", "82M", "111–358ms GPU", "Limited"],
  ["Emotive TTS (Brivva)", "3B", "unknown", "Yes — emotions"],
];

export const LIPSYNC_ROWS: Row[] = [
  ["✓ Wav2Lip + GFPGAN (default)", "Batched 16/fr", "TBD", "Fast, lower quality"],
  ["MuseTalk v1.5", "Per-frame UNet", "~188ms/frame", "Higher quality, 5x slow"],
];

// --- gap analysis ---

export const GAP_ITEMS: GapItem[] = [
  { label: "Translate", currentMs: 170, targetMs: 50 },
  { label: "TTS", currentMs: 539, targetMs: 200 },
  { label: "Lip-sync", currentMs: 0, targetMs: 50, note: "TBD" },
  { label: "Overhead", currentMs: 11, targetMs: 10 },
];

// --- roadmap & known issues ---

export const ROADMAP: Row[] = [
  ["Self-hosted pipeline (Rust)", "−1700ms avg", "Done ✓"],
  ["STT wrapper (clean events)", "−complexity", "Done ✓"],
  ["Parallel translations per lang", "—", "Done ✓"],
  ["GPU inference (NLLB on A10G)", "−5x latency", "Done ✓"],
  ["Wav2Lip + GFPGAN lip-sync", "visual quality", "Built, testing"],
  ["Voice cloning (ElevenLabs IVC)", "host voice preservation", "Not working"],
  ["Emotion-conditioned TTS", "expressive translation", "Not working"],
  ["Streaming TTS playback", "−100ms perceived", "Medium"],
  ["Co-locate all models on one GPU", "−20ms network", "Brivva infra"],
  ["End-to-end speech-to-speech", "paradigm shift", "Brivva's R&D goal"],
];

export const KNOWN_ISSUES: Row[] = [
  ["Voice cloning", "ElevenLabs /v1/voices/add returns error — likely API key tier or PCM format issue. Falls back to default per-language voices."],
  ["Emotion/prosody TTS", "stt-wrapper prosody extraction code exists but style_params not reaching ElevenLabs — always uses default voice_settings."],
  ["Lip-sync frozen body", "Wav2Lip/MuseTalk only modify mouth on single frame. Host body/gestures freeze during 2-5s processing. Industry-wide limitation."],
];

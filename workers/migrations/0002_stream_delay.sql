-- Per-stream output delay (ms). Fargate holds the original host media for
-- this long before pushing it to the RTMP target, giving STT+translate+TTS
-- a window to produce the translated audio that overlays at emit time.
-- Different target languages have different STT+TTS latencies, so the
-- frontend can tune each stream independently.
ALTER TABLE streams ADD COLUMN delay_ms INTEGER NOT NULL DEFAULT 2000;

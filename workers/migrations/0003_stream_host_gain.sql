-- Per-stream ducking level: multiplied against the delayed host audio before
-- it's mixed with the translated TTS. 1.0 = full volume (natural for source-
-- language streams), 0.2 = quiet underlay (natural for target-language streams
-- where the translated TTS should be the dominant voice).
ALTER TABLE streams ADD COLUMN host_gain REAL NOT NULL DEFAULT 0.2;

-- Voice clone language hint. Passed to ElevenLabs /v1/voices/add as a label
-- at clone time so the model picks a language-appropriate speaker profile
-- (fixes the April 2026 Indian-accent regression on Korean host audio).
ALTER TABLE voices ADD COLUMN source_lang TEXT;

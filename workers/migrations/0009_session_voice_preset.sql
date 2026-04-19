-- Per-session voice preset. Lets hosts pick between their cloned voice and
-- the library defaults without having to re-record. Values:
--   'cloned' → use sessions.voice_id (existing cloned voice row)
--   'female' → Lang::voice_id_female() default library voice
--   'male'   → Lang::voice_id_male() default library voice
-- Default 'female' keeps historical behaviour: `Lang::voice_id()` already
-- returned the female defaults; existing sessions stay on the same voices.
ALTER TABLE sessions ADD COLUMN voice_preset TEXT NOT NULL DEFAULT 'female';

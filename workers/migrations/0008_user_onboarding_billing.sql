-- Onboarding + billing surface on the user row.
--
-- `onboarding_completed_at` gates the in-app "first-run" flow — FE reads the
-- flag instead of second-guessing from presence of a voice/channel.
-- `active_voice_id` is the current voice clone (voices POST is upsert; old
-- row + ElevenLabs voice are deleted when a new one takes over).
-- `billing_tier` discriminates self-serve vs B2B; default is self-serve so
-- any existing row continues to get the same treatment.
-- `bills_to` is free-text (invoice target company / person) for B2B clients;
-- see docs/b2b-onboarding-notes.md for the ops workflow that sets it.
ALTER TABLE users ADD COLUMN onboarding_completed_at INTEGER;
ALTER TABLE users ADD COLUMN active_voice_id TEXT;
ALTER TABLE users ADD COLUMN billing_tier TEXT NOT NULL DEFAULT 'self_serve';
ALTER TABLE users ADD COLUMN bills_to TEXT;

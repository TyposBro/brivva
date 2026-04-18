ALTER TABLE sessions ADD COLUMN live_session_id TEXT;

UPDATE sessions
SET live_session_id = room_id
WHERE room_id IS NOT NULL;

-- Google sign-in surfaces email / name / picture from the `userinfo` endpoint.
-- Nullable because pre-sign-in users (seeded from the frontend UUID) have no
-- Google profile attached.
ALTER TABLE users ADD COLUMN email TEXT;
ALTER TABLE users ADD COLUMN name TEXT;
ALTER TABLE users ADD COLUMN picture TEXT;

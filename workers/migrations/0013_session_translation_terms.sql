-- Optional host-provided product/brand/offer glossary for Soniox translation
-- context. Null means the host skipped this setup hint.
ALTER TABLE sessions ADD COLUMN translation_terms TEXT;

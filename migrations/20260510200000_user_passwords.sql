-- Password-based authentication column.
-- NULL for users who signed up via GitHub OAuth only.
ALTER TABLE users ADD COLUMN password_hash TEXT;

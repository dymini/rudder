-- First-run setup could race and create more than one owner. Keep the oldest
-- owner, then let the database refuse a second one.
UPDATE users
SET is_owner = FALSE
WHERE is_owner
  AND id <> (
    SELECT id
    FROM users
    WHERE is_owner
    ORDER BY created_at ASC, id ASC
    LIMIT 1
  );

CREATE UNIQUE INDEX users_one_owner ON users (is_owner) WHERE is_owner;

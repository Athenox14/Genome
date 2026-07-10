CREATE TABLE IF NOT EXISTS repo_secrets (
    id TEXT PRIMARY KEY NOT NULL,
    repo_id TEXT NOT NULL REFERENCES repositories (id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    encrypted_value BLOB NOT NULL,
    created_at TEXT NOT NULL
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_repo_secrets_repo_name ON repo_secrets (repo_id, name);

CREATE TABLE IF NOT EXISTS repo_mirrors (
    id TEXT PRIMARY KEY NOT NULL,
    repo_id TEXT NOT NULL UNIQUE REFERENCES repositories (id) ON DELETE CASCADE,
    remote_url TEXT NOT NULL,
    last_synced_at TEXT,
    sync_interval_minutes INTEGER NOT NULL DEFAULT 60,
    created_at TEXT NOT NULL
);

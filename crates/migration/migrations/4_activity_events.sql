CREATE TABLE IF NOT EXISTS activity_events (
    id TEXT PRIMARY KEY NOT NULL,
    repo_id TEXT REFERENCES repositories (id) ON DELETE CASCADE,
    actor_id TEXT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    kind TEXT NOT NULL,
    summary TEXT NOT NULL,
    created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_activity_events_repo ON activity_events (repo_id);
CREATE INDEX IF NOT EXISTS idx_activity_events_created_at ON activity_events (created_at);

CREATE TABLE IF NOT EXISTS runner_jobs (
    id TEXT PRIMARY KEY NOT NULL,
    kind TEXT NOT NULL,
    repo_id TEXT,
    workflow_run_id TEXT,
    payload TEXT NOT NULL,
    status TEXT NOT NULL,
    claimed_by TEXT,
    claimed_at TEXT,
    created_at TEXT NOT NULL,
    finished_at TEXT
);

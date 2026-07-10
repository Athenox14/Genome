PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS users (
    id TEXT PRIMARY KEY NOT NULL,
    username TEXT NOT NULL,
    email TEXT NOT NULL,
    password_hash TEXT NOT NULL,
    is_admin INTEGER NOT NULL DEFAULT 0,
    avatar_url TEXT,
    created_at TEXT NOT NULL,
    totp_secret TEXT,
    totp_enabled INTEGER NOT NULL DEFAULT 0,
    deactivated_at TEXT
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_users_username ON users (username);
CREATE UNIQUE INDEX IF NOT EXISTS idx_users_email ON users (email);

CREATE TABLE IF NOT EXISTS ssh_keys (
    id TEXT PRIMARY KEY NOT NULL,
    user_id TEXT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    title TEXT NOT NULL,
    public_key TEXT NOT NULL,
    fingerprint TEXT NOT NULL,
    created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_ssh_keys_user ON ssh_keys (user_id);

CREATE TABLE IF NOT EXISTS organizations (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL,
    description TEXT,
    created_at TEXT NOT NULL
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_organizations_name ON organizations (name);

CREATE TABLE IF NOT EXISTS org_members (
    org_id TEXT NOT NULL REFERENCES organizations (id) ON DELETE CASCADE,
    user_id TEXT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    role TEXT NOT NULL,
    PRIMARY KEY (org_id, user_id)
);

CREATE TABLE IF NOT EXISTS repositories (
    id TEXT PRIMARY KEY NOT NULL,
    owner_type TEXT NOT NULL,
    owner_id TEXT NOT NULL,
    name TEXT NOT NULL,
    description TEXT,
    is_private INTEGER NOT NULL DEFAULT 0,
    default_branch TEXT NOT NULL DEFAULT 'main',
    created_at TEXT NOT NULL,
    forked_from_id TEXT REFERENCES repositories (id) ON DELETE SET NULL
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_repositories_owner_name ON repositories (owner_id, owner_type, name);

CREATE TABLE IF NOT EXISTS repo_collaborators (
    repo_id TEXT NOT NULL REFERENCES repositories (id) ON DELETE CASCADE,
    user_id TEXT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    permission TEXT NOT NULL,
    PRIMARY KEY (repo_id, user_id)
);

CREATE TABLE IF NOT EXISTS labels (
    id TEXT PRIMARY KEY NOT NULL,
    repo_id TEXT NOT NULL REFERENCES repositories (id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    color TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_labels_repo ON labels (repo_id);

CREATE TABLE IF NOT EXISTS milestones (
    id TEXT PRIMARY KEY NOT NULL,
    repo_id TEXT NOT NULL REFERENCES repositories (id) ON DELETE CASCADE,
    title TEXT NOT NULL,
    description TEXT,
    due_date TEXT,
    state TEXT NOT NULL DEFAULT 'open',
    created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_milestones_repo ON milestones (repo_id);

CREATE TABLE IF NOT EXISTS issues (
    id TEXT PRIMARY KEY NOT NULL,
    repo_id TEXT NOT NULL REFERENCES repositories (id) ON DELETE CASCADE,
    number INTEGER NOT NULL,
    title TEXT NOT NULL,
    body TEXT,
    author_id TEXT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    state TEXT NOT NULL DEFAULT 'open',
    created_at TEXT NOT NULL,
    closed_at TEXT,
    milestone_id TEXT REFERENCES milestones (id) ON DELETE SET NULL
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_issues_repo_number ON issues (repo_id, number);

CREATE TABLE IF NOT EXISTS issue_labels (
    issue_id TEXT NOT NULL REFERENCES issues (id) ON DELETE CASCADE,
    label_id TEXT NOT NULL REFERENCES labels (id) ON DELETE CASCADE,
    PRIMARY KEY (issue_id, label_id)
);

CREATE TABLE IF NOT EXISTS issue_comments (
    id TEXT PRIMARY KEY NOT NULL,
    issue_id TEXT NOT NULL REFERENCES issues (id) ON DELETE CASCADE,
    author_id TEXT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    body TEXT NOT NULL,
    created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_issue_comments_issue ON issue_comments (issue_id);

CREATE TABLE IF NOT EXISTS pull_requests (
    id TEXT PRIMARY KEY NOT NULL,
    repo_id TEXT NOT NULL REFERENCES repositories (id) ON DELETE CASCADE,
    number INTEGER NOT NULL,
    title TEXT NOT NULL,
    body TEXT,
    author_id TEXT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    source_branch TEXT NOT NULL,
    target_branch TEXT NOT NULL,
    state TEXT NOT NULL DEFAULT 'open',
    created_at TEXT NOT NULL,
    merged_at TEXT
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_pull_requests_repo_number ON pull_requests (repo_id, number);

CREATE TABLE IF NOT EXISTS pr_reviews (
    id TEXT PRIMARY KEY NOT NULL,
    pr_id TEXT NOT NULL REFERENCES pull_requests (id) ON DELETE CASCADE,
    reviewer_id TEXT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    state TEXT NOT NULL,
    body TEXT,
    created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_pr_reviews_pr ON pr_reviews (pr_id);

CREATE TABLE IF NOT EXISTS pr_review_comments (
    id TEXT PRIMARY KEY NOT NULL,
    review_id TEXT NOT NULL REFERENCES pr_reviews (id) ON DELETE CASCADE,
    file_path TEXT NOT NULL,
    line_number INTEGER NOT NULL,
    body TEXT NOT NULL,
    created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_pr_review_comments_review ON pr_review_comments (review_id);

CREATE TABLE IF NOT EXISTS webhooks (
    id TEXT PRIMARY KEY NOT NULL,
    repo_id TEXT NOT NULL REFERENCES repositories (id) ON DELETE CASCADE,
    target_url TEXT NOT NULL,
    secret TEXT NOT NULL,
    events TEXT NOT NULL,
    active INTEGER NOT NULL DEFAULT 1
);
CREATE INDEX IF NOT EXISTS idx_webhooks_repo ON webhooks (repo_id);

CREATE TABLE IF NOT EXISTS workflow_runs (
    id TEXT PRIMARY KEY NOT NULL,
    repo_id TEXT NOT NULL REFERENCES repositories (id) ON DELETE CASCADE,
    workflow_name TEXT NOT NULL,
    commit_sha TEXT NOT NULL,
    event TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'queued',
    started_at TEXT,
    finished_at TEXT
);
CREATE INDEX IF NOT EXISTS idx_workflow_runs_repo ON workflow_runs (repo_id);

CREATE TABLE IF NOT EXISTS workflow_jobs (
    id TEXT PRIMARY KEY NOT NULL,
    run_id TEXT NOT NULL REFERENCES workflow_runs (id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'queued',
    logs_url TEXT
);
CREATE INDEX IF NOT EXISTS idx_workflow_jobs_run ON workflow_jobs (run_id);

CREATE TABLE IF NOT EXISTS workflow_artifacts (
    id TEXT PRIMARY KEY NOT NULL,
    run_id TEXT NOT NULL REFERENCES workflow_runs (id) ON DELETE CASCADE,
    job_id TEXT REFERENCES workflow_jobs (id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    file_path TEXT NOT NULL,
    size_bytes INTEGER NOT NULL,
    created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_workflow_artifacts_run ON workflow_artifacts (run_id);

CREATE TABLE IF NOT EXISTS dev_workspaces (
    id TEXT PRIMARY KEY NOT NULL,
    owner_id TEXT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    repo_id TEXT REFERENCES repositories (id) ON DELETE SET NULL,
    name TEXT NOT NULL,
    image TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'starting',
    container_id TEXT,
    created_at TEXT NOT NULL,
    auto_stop_minutes INTEGER,
    last_activity_at TEXT
);
CREATE INDEX IF NOT EXISTS idx_dev_workspaces_owner ON dev_workspaces (owner_id);

CREATE TABLE IF NOT EXISTS access_tokens (
    id TEXT PRIMARY KEY NOT NULL,
    user_id TEXT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    token_hash TEXT NOT NULL,
    name TEXT NOT NULL,
    scopes TEXT NOT NULL,
    expires_at TEXT
);
CREATE INDEX IF NOT EXISTS idx_access_tokens_user ON access_tokens (user_id);

CREATE TABLE IF NOT EXISTS branch_protection_rules (
    id TEXT PRIMARY KEY NOT NULL,
    repo_id TEXT NOT NULL REFERENCES repositories (id) ON DELETE CASCADE,
    branch_pattern TEXT NOT NULL,
    require_reviews_count INTEGER NOT NULL DEFAULT 0,
    require_status_checks INTEGER NOT NULL DEFAULT 0,
    block_force_push INTEGER NOT NULL DEFAULT 1,
    created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_branch_protection_rules_repo ON branch_protection_rules (repo_id);

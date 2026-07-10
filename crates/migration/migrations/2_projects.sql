CREATE TABLE IF NOT EXISTS projects (
    id TEXT PRIMARY KEY NOT NULL,
    repo_id TEXT NOT NULL REFERENCES repositories (id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_projects_repo ON projects (repo_id);

CREATE TABLE IF NOT EXISTS project_columns (
    id TEXT PRIMARY KEY NOT NULL,
    project_id TEXT NOT NULL REFERENCES projects (id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    position INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_project_columns_project ON project_columns (project_id);

CREATE TABLE IF NOT EXISTS project_cards (
    id TEXT PRIMARY KEY NOT NULL,
    column_id TEXT NOT NULL REFERENCES project_columns (id) ON DELETE CASCADE,
    issue_id TEXT REFERENCES issues (id) ON DELETE CASCADE,
    pull_request_id TEXT REFERENCES pull_requests (id) ON DELETE CASCADE,
    position INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_project_cards_column ON project_cards (column_id);

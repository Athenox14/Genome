CREATE TABLE IF NOT EXISTS packages (
    id TEXT PRIMARY KEY NOT NULL,
    repo_id TEXT REFERENCES repositories (id) ON DELETE SET NULL,
    owner_id TEXT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    version TEXT NOT NULL,
    package_type TEXT NOT NULL,
    file_path TEXT NOT NULL,
    size_bytes INTEGER NOT NULL,
    created_at TEXT NOT NULL
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_packages_owner_name_version ON packages (owner_id, name, version);

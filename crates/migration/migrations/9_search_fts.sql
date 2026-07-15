-- Real full-text search, replacing the substring `LIKE` matching that
-- QueryRoot::search used to be limited to. FTS5 tables here are kept as
-- plain (non "external content") virtual tables with an UNINDEXED `id`
-- column, rather than linked via `content_rowid`, because the base
-- tables key on a TEXT UUID `id`/`repo_id` and FTS5's content_rowid
-- linkage requires an INTEGER rowid alias -- an explicit UNINDEXED
-- column plus triggers is simpler and avoids that mismatch entirely.

CREATE VIRTUAL TABLE IF NOT EXISTS repositories_fts USING fts5(
    id UNINDEXED,
    name,
    description
);

CREATE TRIGGER IF NOT EXISTS repositories_fts_ai AFTER INSERT ON repositories BEGIN
    INSERT INTO repositories_fts (id, name, description)
    VALUES (new.id, new.name, new.description);
END;

CREATE TRIGGER IF NOT EXISTS repositories_fts_ad AFTER DELETE ON repositories BEGIN
    DELETE FROM repositories_fts WHERE id = old.id;
END;

CREATE TRIGGER IF NOT EXISTS repositories_fts_au AFTER UPDATE ON repositories BEGIN
    DELETE FROM repositories_fts WHERE id = old.id;
    INSERT INTO repositories_fts (id, name, description)
    VALUES (new.id, new.name, new.description);
END;

-- Backfill rows that existed before this migration.
INSERT INTO repositories_fts (id, name, description)
SELECT id, name, description FROM repositories;

CREATE VIRTUAL TABLE IF NOT EXISTS issues_fts USING fts5(
    id UNINDEXED,
    repo_id UNINDEXED,
    title,
    body
);

CREATE TRIGGER IF NOT EXISTS issues_fts_ai AFTER INSERT ON issues BEGIN
    INSERT INTO issues_fts (id, repo_id, title, body)
    VALUES (new.id, new.repo_id, new.title, new.body);
END;

CREATE TRIGGER IF NOT EXISTS issues_fts_ad AFTER DELETE ON issues BEGIN
    DELETE FROM issues_fts WHERE id = old.id;
END;

CREATE TRIGGER IF NOT EXISTS issues_fts_au AFTER UPDATE ON issues BEGIN
    DELETE FROM issues_fts WHERE id = old.id;
    INSERT INTO issues_fts (id, repo_id, title, body)
    VALUES (new.id, new.repo_id, new.title, new.body);
END;

INSERT INTO issues_fts (id, repo_id, title, body)
SELECT id, repo_id, title, body FROM issues;

CREATE VIRTUAL TABLE IF NOT EXISTS users_fts USING fts5(
    id UNINDEXED,
    username
);

CREATE TRIGGER IF NOT EXISTS users_fts_ai AFTER INSERT ON users BEGIN
    INSERT INTO users_fts (id, username) VALUES (new.id, new.username);
END;

CREATE TRIGGER IF NOT EXISTS users_fts_ad AFTER DELETE ON users BEGIN
    DELETE FROM users_fts WHERE id = old.id;
END;

CREATE TRIGGER IF NOT EXISTS users_fts_au AFTER UPDATE ON users BEGIN
    DELETE FROM users_fts WHERE id = old.id;
    INSERT INTO users_fts (id, username) VALUES (new.id, new.username);
END;

INSERT INTO users_fts (id, username)
SELECT id, username FROM users;

-- Code/file-content search. Unlike the tables above this has no source
-- table to trigger off of -- it's populated by the application after
-- each push (see `index_repo_code` in crates/server), re-indexing the
-- full tree of the default branch's new tip and replacing that repo's
-- previous rows. There is deliberately no UNIQUE constraint beyond what
-- the app enforces by deleting-then-reinserting per repo_id.
CREATE VIRTUAL TABLE IF NOT EXISTS code_search_fts USING fts5(
    repo_id UNINDEXED,
    path UNINDEXED,
    content
);

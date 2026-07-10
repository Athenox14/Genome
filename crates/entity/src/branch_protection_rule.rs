//! `branch_protection_rules` table.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Model {
    pub id: Uuid,
    pub repo_id: Uuid,
    pub branch_pattern: String,
    pub require_reviews_count: i32,
    pub require_status_checks: bool,
    pub block_force_push: bool,
    pub created_at: DateTime<Utc>,
}

impl From<&mut hiqlite::Row<'_>> for Model {
    /// See the equivalent impl on `entity::repository::Model` for why this
    /// is needed (remote Hiqlite clients require `query_map`, not `query_as`).
    fn from(row: &mut hiqlite::Row<'_>) -> Self {
        Self {
            id: Uuid::parse_str(&row.get::<String>("id")).expect("valid id UUID"),
            repo_id: Uuid::parse_str(&row.get::<String>("repo_id")).expect("valid repo_id UUID"),
            branch_pattern: row.get("branch_pattern"),
            require_reviews_count: row.get::<i64>("require_reviews_count") as i32,
            require_status_checks: row.get::<i64>("require_status_checks") != 0,
            block_force_push: row.get::<i64>("block_force_push") != 0,
            created_at: DateTime::parse_from_rfc3339(&row.get::<String>("created_at"))
                .expect("valid created_at timestamp")
                .with_timezone(&Utc),
        }
    }
}

/// Simple glob matching supporting a literal branch name (e.g. "main") or a
/// trailing wildcard segment (e.g. "release/*"). Not a full glob
/// implementation, but enough for common branch-protection patterns.
pub fn branch_matches_pattern(pattern: &str, branch: &str) -> bool {
    if pattern == branch {
        return true;
    }
    if let Some(prefix) = pattern.strip_suffix('*') {
        return branch.starts_with(prefix);
    }
    false
}

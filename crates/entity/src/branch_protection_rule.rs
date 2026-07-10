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

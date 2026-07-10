//! `activity_events` table.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Model {
    pub id: Uuid,
    pub repo_id: Option<Uuid>,
    pub actor_id: Uuid,
    pub kind: String,
    pub summary: String,
    pub created_at: DateTime<Utc>,
}

pub mod kind {
    pub const PUSH: &str = "push";
    pub const ISSUE_OPENED: &str = "issue_opened";
    pub const PR_OPENED: &str = "pr_opened";
    pub const PR_MERGED: &str = "pr_merged";
    pub const PR_CLOSED: &str = "pr_closed";
    pub const ISSUE_CLOSED: &str = "issue_closed";
    pub const REPO_CREATED: &str = "repo_created";
}

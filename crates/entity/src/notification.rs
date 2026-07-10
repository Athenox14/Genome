//! `notifications` table.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Model {
    pub id: Uuid,
    pub user_id: Uuid,
    pub kind: String,
    pub repo_id: Uuid,
    /// Polymorphic reference to the issue or pull request this notification
    /// concerns. No FK constraint since it can point at either table.
    pub subject_id: Uuid,
    pub message: String,
    pub read_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

pub mod kind {
    pub const ISSUE_COMMENT: &str = "issue_comment";
    pub const PR_REVIEW: &str = "pr_review";
    pub const MENTION: &str = "mention";
    pub const PR_MERGED: &str = "pr_merged";
}

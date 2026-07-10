//! `pull_requests` table.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Model {
    pub id: Uuid,
    pub repo_id: Uuid,
    pub number: i32,
    pub title: String,
    pub body: Option<String>,
    pub author_id: Uuid,
    pub source_branch: String,
    pub target_branch: String,
    pub state: String,
    pub created_at: DateTime<Utc>,
    pub merged_at: Option<DateTime<Utc>>,
}

pub mod state {
    pub const OPEN: &str = "open";
    pub const CLOSED: &str = "closed";
    pub const MERGED: &str = "merged";
}

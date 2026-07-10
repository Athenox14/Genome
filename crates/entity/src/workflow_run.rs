//! `workflow_runs` table.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Model {
    pub id: Uuid,
    pub repo_id: Uuid,
    pub workflow_name: String,
    pub commit_sha: String,
    pub event: String,
    pub status: String,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
}

pub mod status {
    pub const QUEUED: &str = "queued";
    pub const RUNNING: &str = "running";
    pub const SUCCESS: &str = "success";
    pub const FAILURE: &str = "failure";
    pub const CANCELLED: &str = "cancelled";
}

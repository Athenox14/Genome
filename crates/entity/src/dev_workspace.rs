//! `dev_workspaces` table.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Model {
    pub id: Uuid,
    pub owner_id: Uuid,
    pub repo_id: Option<Uuid>,
    pub name: String,
    pub image: String,
    pub status: String,
    pub container_id: Option<String>,
    pub created_at: DateTime<Utc>,
    pub auto_stop_minutes: Option<i32>,
    pub last_activity_at: Option<DateTime<Utc>>,
}

pub mod status {
    pub const STARTING: &str = "starting";
    pub const RUNNING: &str = "running";
    pub const STOPPED: &str = "stopped";
}

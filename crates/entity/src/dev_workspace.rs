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
    /// Free-form identifier self-reported by the standalone `runner`
    /// process hosting this workspace, if any. `None` means it's hosted
    /// directly by `server`'s own Docker daemon (the default/only path
    /// before runner-hosted dev workspaces existed).
    pub runner_id: Option<String>,
}

pub mod status {
    pub const STARTING: &str = "starting";
    pub const RUNNING: &str = "running";
    pub const STOPPED: &str = "stopped";
    /// Enqueued as a `runner_jobs` row (kind `dev_workspace_action`,
    /// action `create`) but not yet claimed/created by a runner.
    pub const PENDING_RUNNER: &str = "pending_runner";
    /// The runner failed to create the workspace's container.
    pub const ERROR: &str = "error";
}

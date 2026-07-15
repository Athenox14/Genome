//! `runner_jobs` table.
//!
//! Queue of jobs claimable by standalone `crates/runner` binaries, polling
//! over HTTP (`POST /runner/claim`). Jobs that the server already executes
//! in-process (`actions::Executor::run_job` called directly from
//! `crates/server`/`crates/graphql-api`) are still recorded here for
//! history, but with `status::HANDLED_INPROCESS` rather than `QUEUED` --
//! `/runner/claim` only ever selects `QUEUED` rows, so those can never be
//! picked up and double-executed by a connected runner.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Model {
    pub id: Uuid,
    pub kind: String,
    pub repo_id: Option<Uuid>,
    pub workflow_run_id: Option<Uuid>,
    /// Raw JSON payload (e.g. serialized `actions::Workflow` job + repo
    /// archive reference) -- caller must `serde_json::from_str`/`to_string`.
    pub payload: String,
    pub status: String,
    pub claimed_by: Option<String>,
    pub claimed_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    /// Raw JSON result reported back via `POST /runner/jobs/:id/complete`,
    /// e.g. `{"container_id": "...", "runner_id": "..."}` for a `create`
    /// dev-workspace action, or `{"output": "..."}` for `exec`. `None` for
    /// CI jobs (which don't report a structured result, only logs) and for
    /// any job not yet completed.
    pub result: Option<String>,
}

pub mod kind {
    pub const CI_JOB: &str = "ci_job";
    /// Dev-workspace hosting via a standalone runner: see
    /// `runner/src/dev_workspace_poll.rs` for the payload/result shape.
    pub const DEV_WORKSPACE_ACTION: &str = "dev_workspace_action";
}

pub mod status {
    pub const QUEUED: &str = "queued";
    pub const CLAIMED: &str = "claimed";
    pub const SUCCESS: &str = "success";
    pub const FAILURE: &str = "failure";
    /// Recorded (not "queued") when the server already ran this job
    /// in-process instead of leaving it for a standalone runner to claim.
    /// `/runner/claim` only selects rows with status `QUEUED`, so a row
    /// inserted with this status is purely a history record and can never
    /// be double-executed by a connected runner.
    pub const HANDLED_INPROCESS: &str = "handled_inprocess";
}

//! `workflow_artifacts` table.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Model {
    pub id: Uuid,
    pub run_id: Uuid,
    pub job_id: Option<Uuid>,
    pub name: String,
    pub file_path: String,
    pub size_bytes: i64,
    pub created_at: DateTime<Utc>,
}

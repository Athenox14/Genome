//! `repo_mirrors` table.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Model {
    pub id: Uuid,
    pub repo_id: Uuid,
    pub remote_url: String,
    pub last_synced_at: Option<DateTime<Utc>>,
    pub sync_interval_minutes: i32,
    pub created_at: DateTime<Utc>,
}

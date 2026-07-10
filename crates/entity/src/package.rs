//! `packages` table.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Model {
    pub id: Uuid,
    pub repo_id: Option<Uuid>,
    pub owner_id: Uuid,
    pub name: String,
    pub version: String,
    pub package_type: String,
    pub file_path: String,
    pub size_bytes: i64,
    pub created_at: DateTime<Utc>,
}

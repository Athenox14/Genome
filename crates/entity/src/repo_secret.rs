//! `repo_secrets` table.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Model {
    pub id: Uuid,
    pub repo_id: Uuid,
    pub name: String,
    pub encrypted_value: Vec<u8>,
    pub created_at: DateTime<Utc>,
}

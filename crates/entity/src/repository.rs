//! `repositories` table.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Model {
    pub id: Uuid,
    /// "user" or "organization"
    pub owner_type: String,
    pub owner_id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub is_private: bool,
    pub default_branch: String,
    pub created_at: DateTime<Utc>,
    pub forked_from_id: Option<Uuid>,
}

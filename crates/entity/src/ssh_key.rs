//! `ssh_keys` table.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Model {
    pub id: Uuid,
    pub user_id: Uuid,
    pub title: String,
    pub public_key: String,
    pub fingerprint: String,
    pub created_at: DateTime<Utc>,
}

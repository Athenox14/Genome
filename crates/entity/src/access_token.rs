//! `access_tokens` table.
//!
//! `scopes` is stored as a `TEXT` column holding serialized JSON (was
//! Postgres `JSONB`). Parse with `serde_json::from_str::<serde_json::Value>`
//! and serialize with `serde_json::to_string` before binding as a param.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Model {
    pub id: Uuid,
    pub user_id: Uuid,
    pub token_hash: String,
    pub name: String,
    pub scopes: String,
    pub expires_at: Option<DateTime<Utc>>,
}

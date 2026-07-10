//! `webhooks` table.
//!
//! `events` is stored as a `TEXT` column holding serialized JSON (was
//! Postgres `JSONB`). Parse with `serde_json::from_str::<serde_json::Value>`
//! and serialize with `serde_json::to_string` before binding as a param.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Model {
    pub id: Uuid,
    pub repo_id: Uuid,
    pub target_url: String,
    pub secret: String,
    pub events: String,
    pub active: bool,
}

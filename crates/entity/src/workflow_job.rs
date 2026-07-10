//! `workflow_jobs` table.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Model {
    pub id: Uuid,
    pub run_id: Uuid,
    pub name: String,
    pub status: String,
    pub logs_url: Option<String>,
}

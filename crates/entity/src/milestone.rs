//! `milestones` table.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Model {
    pub id: Uuid,
    pub repo_id: Uuid,
    pub title: String,
    pub description: Option<String>,
    pub due_date: Option<DateTime<Utc>>,
    pub state: String,
    pub created_at: DateTime<Utc>,
}

pub mod state {
    pub const OPEN: &str = "open";
    pub const CLOSED: &str = "closed";
}

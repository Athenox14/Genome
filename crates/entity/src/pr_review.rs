//! `pr_reviews` table.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Model {
    pub id: Uuid,
    pub pr_id: Uuid,
    pub reviewer_id: Uuid,
    pub state: String,
    pub body: Option<String>,
    pub created_at: DateTime<Utc>,
}

pub mod state {
    pub const APPROVED: &str = "approved";
    pub const CHANGES_REQUESTED: &str = "changes_requested";
    pub const COMMENTED: &str = "commented";
}

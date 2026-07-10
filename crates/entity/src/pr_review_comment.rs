//! `pr_review_comments` table.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Model {
    pub id: Uuid,
    pub review_id: Uuid,
    pub file_path: String,
    pub line_number: i32,
    pub body: String,
    pub created_at: DateTime<Utc>,
}

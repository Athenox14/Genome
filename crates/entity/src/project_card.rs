//! `project_cards` table.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Model {
    pub id: Uuid,
    pub column_id: Uuid,
    pub issue_id: Option<Uuid>,
    pub pull_request_id: Option<Uuid>,
    pub position: i32,
}

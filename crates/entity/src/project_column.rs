//! `project_columns` table.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Model {
    pub id: Uuid,
    pub project_id: Uuid,
    pub name: String,
    pub position: i32,
}

//! `repo_collaborators` table. Composite primary key `(repo_id, user_id)`.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Model {
    pub repo_id: Uuid,
    pub user_id: Uuid,
    pub permission: String,
}

pub mod permission {
    pub const READ: &str = "read";
    pub const WRITE: &str = "write";
    pub const ADMIN: &str = "admin";
}

//! `org_members` table. Composite primary key `(org_id, user_id)`.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Model {
    pub org_id: Uuid,
    pub user_id: Uuid,
    pub role: String,
}

pub mod role {
    pub const OWNER: &str = "owner";
    pub const ADMIN: &str = "admin";
    pub const MEMBER: &str = "member";
}

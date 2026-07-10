//! `users` table.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Model {
    pub id: Uuid,
    pub username: String,
    pub email: String,
    pub password_hash: String,
    pub is_admin: bool,
    pub avatar_url: Option<String>,
    pub created_at: DateTime<Utc>,
    pub totp_secret: Option<String>,
    pub totp_enabled: bool,
    pub deactivated_at: Option<DateTime<Utc>>,
}

//! `repositories` table.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Model {
    pub id: Uuid,
    /// "user" or "organization"
    pub owner_type: String,
    pub owner_id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub is_private: bool,
    pub default_branch: String,
    pub created_at: DateTime<Utc>,
    pub forked_from_id: Option<Uuid>,
}

impl From<&mut hiqlite::Row<'_>> for Model {
    /// Only needed for callers going through `hiqlite::Client::remote(...)`
    /// (e.g. the `check-push-protection` CLI subcommand), where
    /// `query_as::<T>` isn't available and `query_map` (`T: From<&mut Row>`)
    /// must be used instead.
    fn from(row: &mut hiqlite::Row<'_>) -> Self {
        let forked_from_id: Option<String> = row.get("forked_from_id");
        Self {
            id: Uuid::parse_str(&row.get::<String>("id")).expect("valid id UUID"),
            owner_type: row.get("owner_type"),
            owner_id: Uuid::parse_str(&row.get::<String>("owner_id")).expect("valid owner_id UUID"),
            name: row.get("name"),
            description: row.get("description"),
            is_private: row.get::<i64>("is_private") != 0,
            default_branch: row.get("default_branch"),
            created_at: DateTime::parse_from_rfc3339(&row.get::<String>("created_at"))
                .expect("valid created_at timestamp")
                .with_timezone(&Utc),
            forked_from_id: forked_from_id
                .map(|s| Uuid::parse_str(&s).expect("valid forked_from_id UUID")),
        }
    }
}

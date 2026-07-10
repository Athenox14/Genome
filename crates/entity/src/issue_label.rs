//! `issue_labels` table. Composite primary key `(issue_id, label_id)`.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Model {
    pub issue_id: Uuid,
    pub label_id: Uuid,
}

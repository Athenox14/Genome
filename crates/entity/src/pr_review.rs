use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, serde::Serialize, serde::Deserialize)]
#[sea_orm(table_name = "pr_reviews")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub pr_id: Uuid,
    pub reviewer_id: Uuid,
    pub state: String,
    pub body: Option<String>,
    pub created_at: ChronoDateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::pull_request::Entity",
        from = "Column::PrId",
        to = "super::pull_request::Column::Id"
    )]
    PullRequest,
    #[sea_orm(
        belongs_to = "super::user::Entity",
        from = "Column::ReviewerId",
        to = "super::user::Column::Id"
    )]
    Reviewer,
}

impl Related<super::pull_request::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::PullRequest.def()
    }
}
impl Related<super::user::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Reviewer.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}

pub mod state {
    pub const APPROVED: &str = "approved";
    pub const CHANGES_REQUESTED: &str = "changes_requested";
    pub const COMMENTED: &str = "commented";
}

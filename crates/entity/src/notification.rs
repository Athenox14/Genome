use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, serde::Serialize, serde::Deserialize)]
#[sea_orm(table_name = "notifications")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub user_id: Uuid,
    pub kind: String,
    pub repo_id: Uuid,
    /// Polymorphic reference to the issue or pull request this notification
    /// concerns. No FK constraint since it can point at either table.
    pub subject_id: Uuid,
    pub message: String,
    pub read_at: Option<ChronoDateTimeUtc>,
    pub created_at: ChronoDateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::user::Entity",
        from = "Column::UserId",
        to = "super::user::Column::Id"
    )]
    User,
    #[sea_orm(
        belongs_to = "super::repository::Entity",
        from = "Column::RepoId",
        to = "super::repository::Column::Id"
    )]
    Repository,
}

impl Related<super::user::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::User.def()
    }
}
impl Related<super::repository::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Repository.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}

pub mod kind {
    pub const ISSUE_COMMENT: &str = "issue_comment";
    pub const PR_REVIEW: &str = "pr_review";
    pub const MENTION: &str = "mention";
    pub const PR_MERGED: &str = "pr_merged";
}

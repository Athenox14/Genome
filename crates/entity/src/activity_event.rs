use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, serde::Serialize, serde::Deserialize)]
#[sea_orm(table_name = "activity_events")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub repo_id: Option<Uuid>,
    pub actor_id: Uuid,
    pub kind: String,
    pub summary: String,
    pub created_at: ChronoDateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::repository::Entity",
        from = "Column::RepoId",
        to = "super::repository::Column::Id"
    )]
    Repository,
    #[sea_orm(
        belongs_to = "super::user::Entity",
        from = "Column::ActorId",
        to = "super::user::Column::Id"
    )]
    Actor,
}

impl Related<super::repository::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Repository.def()
    }
}
impl Related<super::user::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Actor.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}

pub mod kind {
    pub const PUSH: &str = "push";
    pub const ISSUE_OPENED: &str = "issue_opened";
    pub const PR_OPENED: &str = "pr_opened";
    pub const PR_MERGED: &str = "pr_merged";
    pub const PR_CLOSED: &str = "pr_closed";
    pub const ISSUE_CLOSED: &str = "issue_closed";
    pub const REPO_CREATED: &str = "repo_created";
}

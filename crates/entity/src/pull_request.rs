use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, serde::Serialize, serde::Deserialize)]
#[sea_orm(table_name = "pull_requests")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub repo_id: Uuid,
    pub number: i32,
    pub title: String,
    pub body: Option<String>,
    pub author_id: Uuid,
    pub source_branch: String,
    pub target_branch: String,
    #[sea_orm(default_value = "open")]
    pub state: String,
    pub created_at: ChronoDateTimeUtc,
    pub merged_at: Option<ChronoDateTimeUtc>,
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
        from = "Column::AuthorId",
        to = "super::user::Column::Id"
    )]
    Author,
    #[sea_orm(has_many = "super::pr_review::Entity")]
    PrReview,
}

impl Related<super::repository::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Repository.def()
    }
}
impl Related<super::user::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Author.def()
    }
}
impl Related<super::pr_review::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::PrReview.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}

pub mod state {
    pub const OPEN: &str = "open";
    pub const CLOSED: &str = "closed";
    pub const MERGED: &str = "merged";
}

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, serde::Serialize, serde::Deserialize)]
#[sea_orm(table_name = "repo_collaborators")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub repo_id: Uuid,
    #[sea_orm(primary_key, auto_increment = false)]
    pub user_id: Uuid,
    pub permission: String,
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
        from = "Column::UserId",
        to = "super::user::Column::Id"
    )]
    User,
}

impl Related<super::repository::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Repository.def()
    }
}
impl Related<super::user::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::User.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}

pub mod permission {
    pub const READ: &str = "read";
    pub const WRITE: &str = "write";
    pub const ADMIN: &str = "admin";
}

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, serde::Serialize, serde::Deserialize)]
#[sea_orm(table_name = "dev_workspaces")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub owner_id: Uuid,
    pub repo_id: Option<Uuid>,
    pub name: String,
    pub image: String,
    #[sea_orm(default_value = "starting")]
    pub status: String,
    pub container_id: Option<String>,
    pub created_at: ChronoDateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::user::Entity",
        from = "Column::OwnerId",
        to = "super::user::Column::Id"
    )]
    Owner,
    #[sea_orm(
        belongs_to = "super::repository::Entity",
        from = "Column::RepoId",
        to = "super::repository::Column::Id"
    )]
    Repository,
}

impl Related<super::user::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Owner.def()
    }
}
impl Related<super::repository::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Repository.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}

pub mod status {
    pub const STARTING: &str = "starting";
    pub const RUNNING: &str = "running";
    pub const STOPPED: &str = "stopped";
}

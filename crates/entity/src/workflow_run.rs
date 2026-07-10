use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, serde::Serialize, serde::Deserialize)]
#[sea_orm(table_name = "workflow_runs")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub repo_id: Uuid,
    pub workflow_name: String,
    pub commit_sha: String,
    pub event: String,
    #[sea_orm(default_value = "queued")]
    pub status: String,
    pub started_at: Option<ChronoDateTimeUtc>,
    pub finished_at: Option<ChronoDateTimeUtc>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::repository::Entity",
        from = "Column::RepoId",
        to = "super::repository::Column::Id"
    )]
    Repository,
    #[sea_orm(has_many = "super::workflow_job::Entity")]
    WorkflowJob,
}

impl Related<super::repository::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Repository.def()
    }
}
impl Related<super::workflow_job::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::WorkflowJob.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}

pub mod status {
    pub const QUEUED: &str = "queued";
    pub const RUNNING: &str = "running";
    pub const SUCCESS: &str = "success";
    pub const FAILURE: &str = "failure";
    pub const CANCELLED: &str = "cancelled";
}

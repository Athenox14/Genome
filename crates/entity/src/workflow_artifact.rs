use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, serde::Serialize, serde::Deserialize)]
#[sea_orm(table_name = "workflow_artifacts")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub run_id: Uuid,
    pub job_id: Option<Uuid>,
    pub name: String,
    pub file_path: String,
    pub size_bytes: i64,
    pub created_at: ChronoDateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::workflow_run::Entity",
        from = "Column::RunId",
        to = "super::workflow_run::Column::Id"
    )]
    WorkflowRun,
    #[sea_orm(
        belongs_to = "super::workflow_job::Entity",
        from = "Column::JobId",
        to = "super::workflow_job::Column::Id"
    )]
    WorkflowJob,
}

impl Related<super::workflow_run::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::WorkflowRun.def()
    }
}
impl Related<super::workflow_job::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::WorkflowJob.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}

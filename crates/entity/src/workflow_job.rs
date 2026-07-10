use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, serde::Serialize, serde::Deserialize)]
#[sea_orm(table_name = "workflow_jobs")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub run_id: Uuid,
    pub name: String,
    #[sea_orm(default_value = "queued")]
    pub status: String,
    pub logs_url: Option<String>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::workflow_run::Entity",
        from = "Column::RunId",
        to = "super::workflow_run::Column::Id"
    )]
    WorkflowRun,
}

impl Related<super::workflow_run::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::WorkflowRun.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}

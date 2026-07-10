use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, serde::Serialize, serde::Deserialize)]
#[sea_orm(table_name = "repositories")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    /// "user" or "organization"
    pub owner_type: String,
    pub owner_id: Uuid,
    pub name: String,
    pub description: Option<String>,
    #[sea_orm(default_value = false)]
    pub is_private: bool,
    #[sea_orm(default_value = "main")]
    pub default_branch: String,
    pub created_at: ChronoDateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "super::repo_collaborator::Entity")]
    RepoCollaborator,
    #[sea_orm(has_many = "super::issue::Entity")]
    Issue,
    #[sea_orm(has_many = "super::label::Entity")]
    Label,
    #[sea_orm(has_many = "super::pull_request::Entity")]
    PullRequest,
    #[sea_orm(has_many = "super::webhook::Entity")]
    Webhook,
    #[sea_orm(has_many = "super::workflow_run::Entity")]
    WorkflowRun,
    #[sea_orm(has_many = "super::dev_workspace::Entity")]
    DevWorkspace,
}

impl Related<super::repo_collaborator::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::RepoCollaborator.def()
    }
}
impl Related<super::issue::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Issue.def()
    }
}
impl Related<super::label::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Label.def()
    }
}
impl Related<super::pull_request::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::PullRequest.def()
    }
}
impl Related<super::webhook::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Webhook.def()
    }
}
impl Related<super::workflow_run::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::WorkflowRun.def()
    }
}
impl Related<super::dev_workspace::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::DevWorkspace.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}

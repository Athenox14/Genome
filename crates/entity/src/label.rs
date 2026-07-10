use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, serde::Serialize, serde::Deserialize)]
#[sea_orm(table_name = "labels")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub repo_id: Uuid,
    pub name: String,
    pub color: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::repository::Entity",
        from = "Column::RepoId",
        to = "super::repository::Column::Id"
    )]
    Repository,
    #[sea_orm(has_many = "super::issue_label::Entity")]
    IssueLabel,
}

impl Related<super::repository::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Repository.def()
    }
}
impl Related<super::issue_label::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::IssueLabel.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}

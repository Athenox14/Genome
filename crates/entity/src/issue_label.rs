use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, serde::Serialize, serde::Deserialize)]
#[sea_orm(table_name = "issue_labels")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub issue_id: Uuid,
    #[sea_orm(primary_key, auto_increment = false)]
    pub label_id: Uuid,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::issue::Entity",
        from = "Column::IssueId",
        to = "super::issue::Column::Id"
    )]
    Issue,
    #[sea_orm(
        belongs_to = "super::label::Entity",
        from = "Column::LabelId",
        to = "super::label::Column::Id"
    )]
    Label,
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

impl ActiveModelBehavior for ActiveModel {}

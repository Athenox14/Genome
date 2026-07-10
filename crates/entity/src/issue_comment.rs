use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, serde::Serialize, serde::Deserialize)]
#[sea_orm(table_name = "issue_comments")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub issue_id: Uuid,
    pub author_id: Uuid,
    pub body: String,
    pub created_at: ChronoDateTimeUtc,
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
        belongs_to = "super::user::Entity",
        from = "Column::AuthorId",
        to = "super::user::Column::Id"
    )]
    Author,
}

impl Related<super::issue::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Issue.def()
    }
}
impl Related<super::user::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Author.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}

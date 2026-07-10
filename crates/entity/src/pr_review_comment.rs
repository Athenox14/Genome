use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, serde::Serialize, serde::Deserialize)]
#[sea_orm(table_name = "pr_review_comments")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub review_id: Uuid,
    pub file_path: String,
    pub line_number: i32,
    pub body: String,
    pub created_at: ChronoDateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::pr_review::Entity",
        from = "Column::ReviewId",
        to = "super::pr_review::Column::Id"
    )]
    PrReview,
}

impl Related<super::pr_review::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::PrReview.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}

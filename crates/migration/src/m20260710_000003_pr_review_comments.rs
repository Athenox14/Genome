use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260710_000003_pr_review_comments"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(PrReviewComments::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(PrReviewComments::Id)
                            .uuid()
                            .not_null()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(PrReviewComments::ReviewId).uuid().not_null())
                    .col(
                        ColumnDef::new(PrReviewComments::FilePath)
                            .string()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(PrReviewComments::LineNumber)
                            .integer()
                            .not_null(),
                    )
                    .col(ColumnDef::new(PrReviewComments::Body).text().not_null())
                    .col(
                        ColumnDef::new(PrReviewComments::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_pr_review_comments_review")
                            .from(PrReviewComments::Table, PrReviewComments::ReviewId)
                            .to(PrReviews::Table, PrReviews::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_pr_review_comments_review")
                    .table(PrReviewComments::Table)
                    .col(PrReviewComments::ReviewId)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(
                Table::drop()
                    .table(PrReviewComments::Table)
                    .if_exists()
                    .to_owned(),
            )
            .await
    }
}

#[derive(DeriveIden)]
enum PrReviewComments {
    Table,
    Id,
    ReviewId,
    FilePath,
    LineNumber,
    Body,
    CreatedAt,
}

#[derive(DeriveIden)]
enum PrReviews {
    Table,
    Id,
}

use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260710_000002_branch_protection_rules"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(BranchProtectionRules::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(BranchProtectionRules::Id)
                            .uuid()
                            .not_null()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(BranchProtectionRules::RepoId)
                            .uuid()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(BranchProtectionRules::BranchPattern)
                            .string()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(BranchProtectionRules::RequireReviewsCount)
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(BranchProtectionRules::RequireStatusChecks)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .col(
                        ColumnDef::new(BranchProtectionRules::BlockForcePush)
                            .boolean()
                            .not_null()
                            .default(true),
                    )
                    .col(
                        ColumnDef::new(BranchProtectionRules::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_branch_protection_rules_repo")
                            .from(
                                BranchProtectionRules::Table,
                                BranchProtectionRules::RepoId,
                            )
                            .to(Repositories::Table, Repositories::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_branch_protection_rules_repo")
                    .table(BranchProtectionRules::Table)
                    .col(BranchProtectionRules::RepoId)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(
                Table::drop()
                    .table(BranchProtectionRules::Table)
                    .if_exists()
                    .to_owned(),
            )
            .await
    }
}

#[derive(DeriveIden)]
enum BranchProtectionRules {
    Table,
    Id,
    RepoId,
    BranchPattern,
    RequireReviewsCount,
    RequireStatusChecks,
    BlockForcePush,
    CreatedAt,
}

#[derive(DeriveIden)]
enum Repositories {
    Table,
    Id,
}

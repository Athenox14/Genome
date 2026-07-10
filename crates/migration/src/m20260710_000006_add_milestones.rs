use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260710_000006_add_milestones"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // milestones
        manager
            .create_table(
                Table::create()
                    .table(Milestones::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Milestones::Id)
                            .uuid()
                            .not_null()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(Milestones::RepoId).uuid().not_null())
                    .col(ColumnDef::new(Milestones::Title).string().not_null())
                    .col(ColumnDef::new(Milestones::Description).text())
                    .col(ColumnDef::new(Milestones::DueDate).timestamp_with_time_zone())
                    .col(
                        ColumnDef::new(Milestones::State)
                            .string()
                            .not_null()
                            .default("open"),
                    )
                    .col(
                        ColumnDef::new(Milestones::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_milestones_repo")
                            .from(Milestones::Table, Milestones::RepoId)
                            .to(Repositories::Table, Repositories::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        // issues.milestone_id
        manager
            .alter_table(
                Table::alter()
                    .table(Issues::Table)
                    .add_column(ColumnDef::new(Issues::MilestoneId).uuid())
                    .to_owned(),
            )
            .await?;
        manager
            .create_foreign_key(
                ForeignKey::create()
                    .name("fk_issues_milestone")
                    .from(Issues::Table, Issues::MilestoneId)
                    .to(Milestones::Table, Milestones::Id)
                    .on_delete(ForeignKeyAction::SetNull)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_foreign_key(
                ForeignKey::drop()
                    .name("fk_issues_milestone")
                    .table(Issues::Table)
                    .to_owned(),
            )
            .await?;
        manager
            .alter_table(
                Table::alter()
                    .table(Issues::Table)
                    .drop_column(Issues::MilestoneId)
                    .to_owned(),
            )
            .await?;
        manager
            .drop_table(Table::drop().table(Milestones::Table).if_exists().to_owned())
            .await?;
        Ok(())
    }
}

#[derive(DeriveIden)]
enum Milestones {
    Table,
    Id,
    RepoId,
    Title,
    Description,
    DueDate,
    State,
    CreatedAt,
}

#[derive(DeriveIden)]
enum Repositories {
    Table,
    Id,
}

#[derive(DeriveIden)]
enum Issues {
    Table,
    MilestoneId,
}

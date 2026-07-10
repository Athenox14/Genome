use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260710_000007_add_projects"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(Projects::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(Projects::Id).uuid().not_null().primary_key())
                    .col(ColumnDef::new(Projects::RepoId).uuid().not_null())
                    .col(ColumnDef::new(Projects::Name).string().not_null())
                    .col(
                        ColumnDef::new(Projects::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_projects_repo")
                            .from(Projects::Table, Projects::RepoId)
                            .to(Repositories::Table, Repositories::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(ProjectColumns::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(ProjectColumns::Id)
                            .uuid()
                            .not_null()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(ProjectColumns::ProjectId).uuid().not_null())
                    .col(ColumnDef::new(ProjectColumns::Name).string().not_null())
                    .col(
                        ColumnDef::new(ProjectColumns::Position)
                            .integer()
                            .not_null(),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_project_columns_project")
                            .from(ProjectColumns::Table, ProjectColumns::ProjectId)
                            .to(Projects::Table, Projects::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(ProjectCards::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(ProjectCards::Id)
                            .uuid()
                            .not_null()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(ProjectCards::ColumnId).uuid().not_null())
                    .col(ColumnDef::new(ProjectCards::IssueId).uuid())
                    .col(ColumnDef::new(ProjectCards::PullRequestId).uuid())
                    .col(ColumnDef::new(ProjectCards::Position).integer().not_null())
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_project_cards_column")
                            .from(ProjectCards::Table, ProjectCards::ColumnId)
                            .to(ProjectColumns::Table, ProjectColumns::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_project_cards_issue")
                            .from(ProjectCards::Table, ProjectCards::IssueId)
                            .to(Issues::Table, Issues::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_project_cards_pull_request")
                            .from(ProjectCards::Table, ProjectCards::PullRequestId)
                            .to(PullRequests::Table, PullRequests::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        for table in [
            ProjectCards::Table.into_iden(),
            ProjectColumns::Table.into_iden(),
            Projects::Table.into_iden(),
        ] {
            manager
                .drop_table(Table::drop().table(table).if_exists().to_owned())
                .await?;
        }
        Ok(())
    }
}

#[derive(DeriveIden)]
enum Repositories {
    Table,
    Id,
}

#[derive(DeriveIden)]
enum Issues {
    Table,
    Id,
}

#[derive(DeriveIden)]
enum PullRequests {
    Table,
    Id,
}

#[derive(DeriveIden)]
enum Projects {
    Table,
    Id,
    RepoId,
    Name,
    CreatedAt,
}

#[derive(DeriveIden)]
enum ProjectColumns {
    Table,
    Id,
    ProjectId,
    Name,
    Position,
}

#[derive(DeriveIden)]
enum ProjectCards {
    Table,
    Id,
    ColumnId,
    IssueId,
    PullRequestId,
    Position,
}

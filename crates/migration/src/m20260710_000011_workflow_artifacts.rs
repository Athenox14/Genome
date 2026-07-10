use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260710_000011_workflow_artifacts"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(WorkflowArtifacts::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(WorkflowArtifacts::Id)
                            .uuid()
                            .not_null()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(WorkflowArtifacts::RunId).uuid().not_null())
                    .col(ColumnDef::new(WorkflowArtifacts::JobId).uuid())
                    .col(ColumnDef::new(WorkflowArtifacts::Name).string().not_null())
                    .col(
                        ColumnDef::new(WorkflowArtifacts::FilePath)
                            .string()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(WorkflowArtifacts::SizeBytes)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(WorkflowArtifacts::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_workflow_artifacts_run")
                            .from(WorkflowArtifacts::Table, WorkflowArtifacts::RunId)
                            .to(WorkflowRuns::Table, WorkflowRuns::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_workflow_artifacts_job")
                            .from(WorkflowArtifacts::Table, WorkflowArtifacts::JobId)
                            .to(WorkflowJobs::Table, WorkflowJobs::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_workflow_artifacts_run")
                    .table(WorkflowArtifacts::Table)
                    .col(WorkflowArtifacts::RunId)
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(
                Table::drop()
                    .table(WorkflowArtifacts::Table)
                    .if_exists()
                    .to_owned(),
            )
            .await
    }
}

#[derive(DeriveIden)]
enum WorkflowArtifacts {
    Table,
    Id,
    RunId,
    JobId,
    Name,
    FilePath,
    SizeBytes,
    CreatedAt,
}

#[derive(DeriveIden)]
enum WorkflowRuns {
    Table,
    Id,
}

#[derive(DeriveIden)]
enum WorkflowJobs {
    Table,
    Id,
}

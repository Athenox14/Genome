use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260710_000012_dev_workspace_autostop"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(DevWorkspaces::Table)
                    .add_column(ColumnDef::new(DevWorkspaces::AutoStopMinutes).integer())
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(DevWorkspaces::Table)
                    .add_column(
                        ColumnDef::new(DevWorkspaces::LastActivityAt).timestamp_with_time_zone(),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(DevWorkspaces::Table)
                    .drop_column(DevWorkspaces::AutoStopMinutes)
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(DevWorkspaces::Table)
                    .drop_column(DevWorkspaces::LastActivityAt)
                    .to_owned(),
            )
            .await
    }
}

#[derive(DeriveIden)]
enum DevWorkspaces {
    Table,
    AutoStopMinutes,
    LastActivityAt,
}

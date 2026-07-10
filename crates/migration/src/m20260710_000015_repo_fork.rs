use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260710_000015_repo_fork"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Repositories::Table)
                    .add_column(ColumnDef::new(Repositories::ForkedFromId).uuid().null())
                    .to_owned(),
            )
            .await?;

        manager
            .create_foreign_key(
                ForeignKey::create()
                    .name("fk_repositories_forked_from_id")
                    .from(Repositories::Table, Repositories::ForkedFromId)
                    .to(Repositories::Table, Repositories::Id)
                    .on_delete(ForeignKeyAction::SetNull)
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_foreign_key(
                ForeignKey::drop()
                    .table(Repositories::Table)
                    .name("fk_repositories_forked_from_id")
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(Repositories::Table)
                    .drop_column(Repositories::ForkedFromId)
                    .to_owned(),
            )
            .await
    }
}

#[derive(DeriveIden)]
enum Repositories {
    Table,
    Id,
    ForkedFromId,
}

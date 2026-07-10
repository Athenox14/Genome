use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260710_000016_repo_mirrors"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(RepoMirrors::Table)
                    .col(ColumnDef::new(RepoMirrors::Id).uuid().not_null().primary_key())
                    .col(ColumnDef::new(RepoMirrors::RepoId).uuid().not_null().unique_key())
                    .col(ColumnDef::new(RepoMirrors::RemoteUrl).string().not_null())
                    .col(ColumnDef::new(RepoMirrors::LastSyncedAt).timestamp_with_time_zone().null())
                    .col(
                        ColumnDef::new(RepoMirrors::SyncIntervalMinutes)
                            .integer()
                            .not_null()
                            .default(60),
                    )
                    .col(
                        ColumnDef::new(RepoMirrors::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_repo_mirrors_repo_id")
                            .from_col(RepoMirrors::RepoId)
                            .to(Repositories::Table, Repositories::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(RepoMirrors::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
enum RepoMirrors {
    Table,
    Id,
    RepoId,
    RemoteUrl,
    LastSyncedAt,
    SyncIntervalMinutes,
    CreatedAt,
}

#[derive(DeriveIden)]
enum Repositories {
    Table,
    Id,
}

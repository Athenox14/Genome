use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260710_000010_repo_secrets"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(RepoSecrets::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(RepoSecrets::Id)
                            .uuid()
                            .not_null()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(RepoSecrets::RepoId).uuid().not_null())
                    .col(ColumnDef::new(RepoSecrets::Name).string().not_null())
                    .col(
                        ColumnDef::new(RepoSecrets::EncryptedValue)
                            .binary()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(RepoSecrets::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_repo_secrets_repo")
                            .from(RepoSecrets::Table, RepoSecrets::RepoId)
                            .to(Repositories::Table, Repositories::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_repo_secrets_repo_name")
                    .table(RepoSecrets::Table)
                    .col(RepoSecrets::RepoId)
                    .col(RepoSecrets::Name)
                    .unique()
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(RepoSecrets::Table).if_exists().to_owned())
            .await
    }
}

#[derive(DeriveIden)]
enum RepoSecrets {
    Table,
    Id,
    RepoId,
    Name,
    EncryptedValue,
    CreatedAt,
}

#[derive(DeriveIden)]
enum Repositories {
    Table,
    Id,
}

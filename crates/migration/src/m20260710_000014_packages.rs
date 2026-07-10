use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260710_000014_packages"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(Packages::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(Packages::Id).uuid().not_null().primary_key())
                    .col(ColumnDef::new(Packages::RepoId).uuid())
                    .col(ColumnDef::new(Packages::OwnerId).uuid().not_null())
                    .col(ColumnDef::new(Packages::Name).string().not_null())
                    .col(ColumnDef::new(Packages::Version).string().not_null())
                    .col(ColumnDef::new(Packages::PackageType).string().not_null())
                    .col(ColumnDef::new(Packages::FilePath).string().not_null())
                    .col(ColumnDef::new(Packages::SizeBytes).big_integer().not_null())
                    .col(
                        ColumnDef::new(Packages::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_packages_repo")
                            .from(Packages::Table, Packages::RepoId)
                            .to(Repositories::Table, Repositories::Id)
                            .on_delete(ForeignKeyAction::SetNull),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_packages_owner")
                            .from(Packages::Table, Packages::OwnerId)
                            .to(Users::Table, Users::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_packages_owner_name_version")
                    .table(Packages::Table)
                    .col(Packages::OwnerId)
                    .col(Packages::Name)
                    .col(Packages::Version)
                    .unique()
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(Packages::Table).if_exists().to_owned())
            .await
    }
}

#[derive(DeriveIden)]
enum Users {
    Table,
    Id,
}

#[derive(DeriveIden)]
enum Repositories {
    Table,
    Id,
}

#[derive(DeriveIden)]
enum Packages {
    Table,
    Id,
    RepoId,
    OwnerId,
    Name,
    Version,
    PackageType,
    FilePath,
    SizeBytes,
    CreatedAt,
}

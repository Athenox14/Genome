use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260710_000009_activity_events"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(ActivityEvents::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(ActivityEvents::Id)
                            .uuid()
                            .not_null()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(ActivityEvents::RepoId).uuid())
                    .col(ColumnDef::new(ActivityEvents::ActorId).uuid().not_null())
                    .col(ColumnDef::new(ActivityEvents::Kind).string().not_null())
                    .col(ColumnDef::new(ActivityEvents::Summary).text().not_null())
                    .col(
                        ColumnDef::new(ActivityEvents::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_activity_events_repo")
                            .from(ActivityEvents::Table, ActivityEvents::RepoId)
                            .to(Repositories::Table, Repositories::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_activity_events_actor")
                            .from(ActivityEvents::Table, ActivityEvents::ActorId)
                            .to(Users::Table, Users::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_activity_events_repo")
                    .table(ActivityEvents::Table)
                    .col(ActivityEvents::RepoId)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_activity_events_created_at")
                    .table(ActivityEvents::Table)
                    .col(ActivityEvents::CreatedAt)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(ActivityEvents::Table).if_exists().to_owned())
            .await
    }
}

#[derive(DeriveIden)]
enum ActivityEvents {
    Table,
    Id,
    RepoId,
    ActorId,
    Kind,
    Summary,
    CreatedAt,
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

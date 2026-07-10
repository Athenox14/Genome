use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260710_000013_oauth2"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(Oauth2Applications::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Oauth2Applications::Id)
                            .uuid()
                            .not_null()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(Oauth2Applications::OwnerId).uuid().not_null())
                    .col(ColumnDef::new(Oauth2Applications::Name).string().not_null())
                    .col(
                        ColumnDef::new(Oauth2Applications::ClientId)
                            .string()
                            .not_null()
                            .unique_key(),
                    )
                    .col(
                        ColumnDef::new(Oauth2Applications::ClientSecretHash)
                            .string()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(Oauth2Applications::RedirectUri)
                            .string()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(Oauth2Applications::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_oauth2_applications_owner")
                            .from(Oauth2Applications::Table, Oauth2Applications::OwnerId)
                            .to(Users::Table, Users::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(Oauth2AuthorizationCodes::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Oauth2AuthorizationCodes::Code)
                            .string()
                            .not_null()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(Oauth2AuthorizationCodes::ApplicationId)
                            .uuid()
                            .not_null(),
                    )
                    .col(ColumnDef::new(Oauth2AuthorizationCodes::UserId).uuid().not_null())
                    .col(
                        ColumnDef::new(Oauth2AuthorizationCodes::RedirectUri)
                            .string()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(Oauth2AuthorizationCodes::ExpiresAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(Oauth2AuthorizationCodes::Used)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_oauth2_auth_codes_application")
                            .from(Oauth2AuthorizationCodes::Table, Oauth2AuthorizationCodes::ApplicationId)
                            .to(Oauth2Applications::Table, Oauth2Applications::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_oauth2_auth_codes_user")
                            .from(Oauth2AuthorizationCodes::Table, Oauth2AuthorizationCodes::UserId)
                            .to(Users::Table, Users::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(Oauth2AccessTokens::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Oauth2AccessTokens::TokenHash)
                            .string()
                            .not_null()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(Oauth2AccessTokens::ApplicationId)
                            .uuid()
                            .not_null(),
                    )
                    .col(ColumnDef::new(Oauth2AccessTokens::UserId).uuid().not_null())
                    .col(ColumnDef::new(Oauth2AccessTokens::Scopes).string().not_null())
                    .col(
                        ColumnDef::new(Oauth2AccessTokens::ExpiresAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_oauth2_access_tokens_application")
                            .from(Oauth2AccessTokens::Table, Oauth2AccessTokens::ApplicationId)
                            .to(Oauth2Applications::Table, Oauth2Applications::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_oauth2_access_tokens_user")
                            .from(Oauth2AccessTokens::Table, Oauth2AccessTokens::UserId)
                            .to(Users::Table, Users::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(Oauth2AccessTokens::Table).if_exists().to_owned())
            .await?;
        manager
            .drop_table(
                Table::drop()
                    .table(Oauth2AuthorizationCodes::Table)
                    .if_exists()
                    .to_owned(),
            )
            .await?;
        manager
            .drop_table(Table::drop().table(Oauth2Applications::Table).if_exists().to_owned())
            .await
    }
}

#[derive(DeriveIden)]
enum Users {
    Table,
    Id,
}

#[derive(DeriveIden)]
enum Oauth2Applications {
    Table,
    Id,
    OwnerId,
    Name,
    ClientId,
    ClientSecretHash,
    RedirectUri,
    CreatedAt,
}

#[derive(DeriveIden)]
enum Oauth2AuthorizationCodes {
    Table,
    Code,
    ApplicationId,
    UserId,
    RedirectUri,
    ExpiresAt,
    Used,
}

#[derive(DeriveIden)]
enum Oauth2AccessTokens {
    Table,
    TokenHash,
    ApplicationId,
    UserId,
    Scopes,
    ExpiresAt,
}

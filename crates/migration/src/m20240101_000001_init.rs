use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20240101_000001_init"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(Users::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(Users::Id).uuid().not_null().primary_key())
                    .col(ColumnDef::new(Users::Username).string().not_null())
                    .col(ColumnDef::new(Users::Email).string().not_null())
                    .col(ColumnDef::new(Users::PasswordHash).string().not_null())
                    .col(
                        ColumnDef::new(Users::IsAdmin)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .col(ColumnDef::new(Users::AvatarUrl).string())
                    .col(
                        ColumnDef::new(Users::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_users_username")
                    .table(Users::Table)
                    .col(Users::Username)
                    .unique()
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_users_email")
                    .table(Users::Table)
                    .col(Users::Email)
                    .unique()
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(SshKeys::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(SshKeys::Id).uuid().not_null().primary_key())
                    .col(ColumnDef::new(SshKeys::UserId).uuid().not_null())
                    .col(ColumnDef::new(SshKeys::Title).string().not_null())
                    .col(ColumnDef::new(SshKeys::PublicKey).text().not_null())
                    .col(ColumnDef::new(SshKeys::Fingerprint).string().not_null())
                    .col(
                        ColumnDef::new(SshKeys::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_ssh_keys_user")
                            .from(SshKeys::Table, SshKeys::UserId)
                            .to(Users::Table, Users::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(Organizations::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Organizations::Id)
                            .uuid()
                            .not_null()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(Organizations::Name).string().not_null())
                    .col(ColumnDef::new(Organizations::Description).text())
                    .col(
                        ColumnDef::new(Organizations::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_organizations_name")
                    .table(Organizations::Table)
                    .col(Organizations::Name)
                    .unique()
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(OrgMembers::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(OrgMembers::OrgId).uuid().not_null())
                    .col(ColumnDef::new(OrgMembers::UserId).uuid().not_null())
                    .col(ColumnDef::new(OrgMembers::Role).string().not_null())
                    .primary_key(
                        Index::create()
                            .col(OrgMembers::OrgId)
                            .col(OrgMembers::UserId),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_org_members_org")
                            .from(OrgMembers::Table, OrgMembers::OrgId)
                            .to(Organizations::Table, Organizations::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_org_members_user")
                            .from(OrgMembers::Table, OrgMembers::UserId)
                            .to(Users::Table, Users::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(Repositories::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Repositories::Id)
                            .uuid()
                            .not_null()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(Repositories::OwnerType)
                            .string()
                            .not_null(),
                    )
                    .col(ColumnDef::new(Repositories::OwnerId).uuid().not_null())
                    .col(ColumnDef::new(Repositories::Name).string().not_null())
                    .col(ColumnDef::new(Repositories::Description).text())
                    .col(
                        ColumnDef::new(Repositories::IsPrivate)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .col(
                        ColumnDef::new(Repositories::DefaultBranch)
                            .string()
                            .not_null()
                            .default("main"),
                    )
                    .col(
                        ColumnDef::new(Repositories::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_repositories_owner_name")
                    .table(Repositories::Table)
                    .col(Repositories::OwnerId)
                    .col(Repositories::OwnerType)
                    .col(Repositories::Name)
                    .unique()
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(RepoCollaborators::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(RepoCollaborators::RepoId).uuid().not_null())
                    .col(ColumnDef::new(RepoCollaborators::UserId).uuid().not_null())
                    .col(
                        ColumnDef::new(RepoCollaborators::Permission)
                            .string()
                            .not_null(),
                    )
                    .primary_key(
                        Index::create()
                            .col(RepoCollaborators::RepoId)
                            .col(RepoCollaborators::UserId),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_repo_collaborators_repo")
                            .from(RepoCollaborators::Table, RepoCollaborators::RepoId)
                            .to(Repositories::Table, Repositories::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_repo_collaborators_user")
                            .from(RepoCollaborators::Table, RepoCollaborators::UserId)
                            .to(Users::Table, Users::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(Labels::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(Labels::Id).uuid().not_null().primary_key())
                    .col(ColumnDef::new(Labels::RepoId).uuid().not_null())
                    .col(ColumnDef::new(Labels::Name).string().not_null())
                    .col(ColumnDef::new(Labels::Color).string().not_null())
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_labels_repo")
                            .from(Labels::Table, Labels::RepoId)
                            .to(Repositories::Table, Repositories::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(Issues::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(Issues::Id).uuid().not_null().primary_key())
                    .col(ColumnDef::new(Issues::RepoId).uuid().not_null())
                    .col(ColumnDef::new(Issues::Number).integer().not_null())
                    .col(ColumnDef::new(Issues::Title).string().not_null())
                    .col(ColumnDef::new(Issues::Body).text())
                    .col(ColumnDef::new(Issues::AuthorId).uuid().not_null())
                    .col(
                        ColumnDef::new(Issues::State)
                            .string()
                            .not_null()
                            .default("open"),
                    )
                    .col(
                        ColumnDef::new(Issues::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .col(ColumnDef::new(Issues::ClosedAt).timestamp_with_time_zone())
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_issues_repo")
                            .from(Issues::Table, Issues::RepoId)
                            .to(Repositories::Table, Repositories::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_issues_author")
                            .from(Issues::Table, Issues::AuthorId)
                            .to(Users::Table, Users::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_issues_repo_number")
                    .table(Issues::Table)
                    .col(Issues::RepoId)
                    .col(Issues::Number)
                    .unique()
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(IssueLabels::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(IssueLabels::IssueId).uuid().not_null())
                    .col(ColumnDef::new(IssueLabels::LabelId).uuid().not_null())
                    .primary_key(
                        Index::create()
                            .col(IssueLabels::IssueId)
                            .col(IssueLabels::LabelId),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_issue_labels_issue")
                            .from(IssueLabels::Table, IssueLabels::IssueId)
                            .to(Issues::Table, Issues::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_issue_labels_label")
                            .from(IssueLabels::Table, IssueLabels::LabelId)
                            .to(Labels::Table, Labels::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(IssueComments::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(IssueComments::Id)
                            .uuid()
                            .not_null()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(IssueComments::IssueId).uuid().not_null())
                    .col(ColumnDef::new(IssueComments::AuthorId).uuid().not_null())
                    .col(ColumnDef::new(IssueComments::Body).text().not_null())
                    .col(
                        ColumnDef::new(IssueComments::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_issue_comments_issue")
                            .from(IssueComments::Table, IssueComments::IssueId)
                            .to(Issues::Table, Issues::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_issue_comments_author")
                            .from(IssueComments::Table, IssueComments::AuthorId)
                            .to(Users::Table, Users::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(PullRequests::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(PullRequests::Id)
                            .uuid()
                            .not_null()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(PullRequests::RepoId).uuid().not_null())
                    .col(ColumnDef::new(PullRequests::Number).integer().not_null())
                    .col(ColumnDef::new(PullRequests::Title).string().not_null())
                    .col(ColumnDef::new(PullRequests::Body).text())
                    .col(ColumnDef::new(PullRequests::AuthorId).uuid().not_null())
                    .col(
                        ColumnDef::new(PullRequests::SourceBranch)
                            .string()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(PullRequests::TargetBranch)
                            .string()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(PullRequests::State)
                            .string()
                            .not_null()
                            .default("open"),
                    )
                    .col(
                        ColumnDef::new(PullRequests::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(PullRequests::MergedAt).timestamp_with_time_zone(),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_pull_requests_repo")
                            .from(PullRequests::Table, PullRequests::RepoId)
                            .to(Repositories::Table, Repositories::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_pull_requests_author")
                            .from(PullRequests::Table, PullRequests::AuthorId)
                            .to(Users::Table, Users::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_pull_requests_repo_number")
                    .table(PullRequests::Table)
                    .col(PullRequests::RepoId)
                    .col(PullRequests::Number)
                    .unique()
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(PrReviews::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(PrReviews::Id)
                            .uuid()
                            .not_null()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(PrReviews::PrId).uuid().not_null())
                    .col(ColumnDef::new(PrReviews::ReviewerId).uuid().not_null())
                    .col(ColumnDef::new(PrReviews::State).string().not_null())
                    .col(ColumnDef::new(PrReviews::Body).text())
                    .col(
                        ColumnDef::new(PrReviews::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_pr_reviews_pr")
                            .from(PrReviews::Table, PrReviews::PrId)
                            .to(PullRequests::Table, PullRequests::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_pr_reviews_reviewer")
                            .from(PrReviews::Table, PrReviews::ReviewerId)
                            .to(Users::Table, Users::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(Webhooks::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Webhooks::Id)
                            .uuid()
                            .not_null()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(Webhooks::RepoId).uuid().not_null())
                    .col(ColumnDef::new(Webhooks::TargetUrl).string().not_null())
                    .col(ColumnDef::new(Webhooks::Secret).string().not_null())
                    .col(ColumnDef::new(Webhooks::Events).json_binary().not_null())
                    .col(
                        ColumnDef::new(Webhooks::Active)
                            .boolean()
                            .not_null()
                            .default(true),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_webhooks_repo")
                            .from(Webhooks::Table, Webhooks::RepoId)
                            .to(Repositories::Table, Repositories::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(WorkflowRuns::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(WorkflowRuns::Id)
                            .uuid()
                            .not_null()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(WorkflowRuns::RepoId).uuid().not_null())
                    .col(
                        ColumnDef::new(WorkflowRuns::WorkflowName)
                            .string()
                            .not_null(),
                    )
                    .col(ColumnDef::new(WorkflowRuns::CommitSha).string().not_null())
                    .col(ColumnDef::new(WorkflowRuns::Event).string().not_null())
                    .col(
                        ColumnDef::new(WorkflowRuns::Status)
                            .string()
                            .not_null()
                            .default("queued"),
                    )
                    .col(
                        ColumnDef::new(WorkflowRuns::StartedAt).timestamp_with_time_zone(),
                    )
                    .col(
                        ColumnDef::new(WorkflowRuns::FinishedAt).timestamp_with_time_zone(),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_workflow_runs_repo")
                            .from(WorkflowRuns::Table, WorkflowRuns::RepoId)
                            .to(Repositories::Table, Repositories::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(WorkflowJobs::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(WorkflowJobs::Id)
                            .uuid()
                            .not_null()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(WorkflowJobs::RunId).uuid().not_null())
                    .col(ColumnDef::new(WorkflowJobs::Name).string().not_null())
                    .col(
                        ColumnDef::new(WorkflowJobs::Status)
                            .string()
                            .not_null()
                            .default("queued"),
                    )
                    .col(ColumnDef::new(WorkflowJobs::LogsUrl).string())
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_workflow_jobs_run")
                            .from(WorkflowJobs::Table, WorkflowJobs::RunId)
                            .to(WorkflowRuns::Table, WorkflowRuns::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(DevWorkspaces::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(DevWorkspaces::Id)
                            .uuid()
                            .not_null()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(DevWorkspaces::OwnerId).uuid().not_null())
                    .col(ColumnDef::new(DevWorkspaces::RepoId).uuid())
                    .col(ColumnDef::new(DevWorkspaces::Name).string().not_null())
                    .col(ColumnDef::new(DevWorkspaces::Image).string().not_null())
                    .col(
                        ColumnDef::new(DevWorkspaces::Status)
                            .string()
                            .not_null()
                            .default("starting"),
                    )
                    .col(ColumnDef::new(DevWorkspaces::ContainerId).string())
                    .col(
                        ColumnDef::new(DevWorkspaces::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_dev_workspaces_owner")
                            .from(DevWorkspaces::Table, DevWorkspaces::OwnerId)
                            .to(Users::Table, Users::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_dev_workspaces_repo")
                            .from(DevWorkspaces::Table, DevWorkspaces::RepoId)
                            .to(Repositories::Table, Repositories::Id)
                            .on_delete(ForeignKeyAction::SetNull),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(AccessTokens::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(AccessTokens::Id)
                            .uuid()
                            .not_null()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(AccessTokens::UserId).uuid().not_null())
                    .col(ColumnDef::new(AccessTokens::TokenHash).string().not_null())
                    .col(ColumnDef::new(AccessTokens::Name).string().not_null())
                    .col(ColumnDef::new(AccessTokens::Scopes).json_binary().not_null())
                    .col(
                        ColumnDef::new(AccessTokens::ExpiresAt).timestamp_with_time_zone(),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_access_tokens_user")
                            .from(AccessTokens::Table, AccessTokens::UserId)
                            .to(Users::Table, Users::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        for table in [
            AccessTokens::Table.into_iden(),
            DevWorkspaces::Table.into_iden(),
            WorkflowJobs::Table.into_iden(),
            WorkflowRuns::Table.into_iden(),
            Webhooks::Table.into_iden(),
            PrReviews::Table.into_iden(),
            PullRequests::Table.into_iden(),
            IssueComments::Table.into_iden(),
            IssueLabels::Table.into_iden(),
            Issues::Table.into_iden(),
            Labels::Table.into_iden(),
            RepoCollaborators::Table.into_iden(),
            Repositories::Table.into_iden(),
            OrgMembers::Table.into_iden(),
            Organizations::Table.into_iden(),
            SshKeys::Table.into_iden(),
            Users::Table.into_iden(),
        ] {
            manager
                .drop_table(Table::drop().table(table).if_exists().to_owned())
                .await?;
        }
        Ok(())
    }
}

#[derive(DeriveIden)]
enum Users {
    Table,
    Id,
    Username,
    Email,
    PasswordHash,
    IsAdmin,
    AvatarUrl,
    CreatedAt,
}

#[derive(DeriveIden)]
enum SshKeys {
    Table,
    Id,
    UserId,
    Title,
    PublicKey,
    Fingerprint,
    CreatedAt,
}

#[derive(DeriveIden)]
enum Organizations {
    Table,
    Id,
    Name,
    Description,
    CreatedAt,
}

#[derive(DeriveIden)]
enum OrgMembers {
    Table,
    OrgId,
    UserId,
    Role,
}

#[derive(DeriveIden)]
enum Repositories {
    Table,
    Id,
    OwnerType,
    OwnerId,
    Name,
    Description,
    IsPrivate,
    DefaultBranch,
    CreatedAt,
}

#[derive(DeriveIden)]
enum RepoCollaborators {
    Table,
    RepoId,
    UserId,
    Permission,
}

#[derive(DeriveIden)]
enum Labels {
    Table,
    Id,
    RepoId,
    Name,
    Color,
}

#[derive(DeriveIden)]
enum Issues {
    Table,
    Id,
    RepoId,
    Number,
    Title,
    Body,
    AuthorId,
    State,
    CreatedAt,
    ClosedAt,
}

#[derive(DeriveIden)]
enum IssueLabels {
    Table,
    IssueId,
    LabelId,
}

#[derive(DeriveIden)]
enum IssueComments {
    Table,
    Id,
    IssueId,
    AuthorId,
    Body,
    CreatedAt,
}

#[derive(DeriveIden)]
enum PullRequests {
    Table,
    Id,
    RepoId,
    Number,
    Title,
    Body,
    AuthorId,
    SourceBranch,
    TargetBranch,
    State,
    CreatedAt,
    MergedAt,
}

#[derive(DeriveIden)]
enum PrReviews {
    Table,
    Id,
    PrId,
    ReviewerId,
    State,
    Body,
    CreatedAt,
}

#[derive(DeriveIden)]
enum Webhooks {
    Table,
    Id,
    RepoId,
    TargetUrl,
    Secret,
    Events,
    Active,
}

#[derive(DeriveIden)]
enum WorkflowRuns {
    Table,
    Id,
    RepoId,
    WorkflowName,
    CommitSha,
    Event,
    Status,
    StartedAt,
    FinishedAt,
}

#[derive(DeriveIden)]
enum WorkflowJobs {
    Table,
    Id,
    RunId,
    Name,
    Status,
    LogsUrl,
}

#[derive(DeriveIden)]
enum DevWorkspaces {
    Table,
    Id,
    OwnerId,
    RepoId,
    Name,
    Image,
    Status,
    ContainerId,
    CreatedAt,
}

#[derive(DeriveIden)]
enum AccessTokens {
    Table,
    Id,
    UserId,
    TokenHash,
    Name,
    Scopes,
    ExpiresAt,
}

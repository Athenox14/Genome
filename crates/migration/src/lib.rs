pub use sea_orm_migration::prelude::*;

mod m20240101_000001_init;
mod m20260710_000002_branch_protection_rules;
mod m20260710_000003_pr_review_comments;
mod m20260710_000004_totp_admin;
mod m20260710_000006_add_milestones;
mod m20260710_000007_add_projects;
mod m20260710_000008_notifications;
mod m20260710_000009_activity_events;
mod m20260710_000010_repo_secrets;
mod m20260710_000011_workflow_artifacts;
mod m20260710_000012_dev_workspace_autostop;
mod m20260710_000013_oauth2;
mod m20260710_000014_packages;
mod m20260710_000015_repo_fork;
mod m20260710_000016_repo_mirrors;

pub struct Migrator;

#[sea_orm_migration::async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(m20240101_000001_init::Migration),
            Box::new(m20260710_000002_branch_protection_rules::Migration),
            Box::new(m20260710_000003_pr_review_comments::Migration),
            Box::new(m20260710_000004_totp_admin::Migration),
            Box::new(m20260710_000006_add_milestones::Migration),
            Box::new(m20260710_000007_add_projects::Migration),
            Box::new(m20260710_000008_notifications::Migration),
            Box::new(m20260710_000009_activity_events::Migration),
            Box::new(m20260710_000010_repo_secrets::Migration),
            Box::new(m20260710_000011_workflow_artifacts::Migration),
            Box::new(m20260710_000012_dev_workspace_autostop::Migration),
            Box::new(m20260710_000013_oauth2::Migration),
            Box::new(m20260710_000014_packages::Migration),
            Box::new(m20260710_000015_repo_fork::Migration),
            Box::new(m20260710_000016_repo_mirrors::Migration),
        ]
    }
}

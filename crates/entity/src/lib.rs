pub mod access_token;
pub mod dev_workspace;
pub mod issue;
pub mod issue_comment;
pub mod issue_label;
pub mod label;
pub mod org_member;
pub mod organization;
pub mod pr_review;
pub mod pull_request;
pub mod repo_collaborator;
pub mod repository;
pub mod ssh_key;
pub mod user;
pub mod webhook;
pub mod workflow_job;
pub mod workflow_run;

pub mod prelude {
    pub use super::access_token::Entity as AccessToken;
    pub use super::dev_workspace::Entity as DevWorkspace;
    pub use super::issue::Entity as Issue;
    pub use super::issue_comment::Entity as IssueComment;
    pub use super::issue_label::Entity as IssueLabel;
    pub use super::label::Entity as Label;
    pub use super::org_member::Entity as OrgMember;
    pub use super::organization::Entity as Organization;
    pub use super::pr_review::Entity as PrReview;
    pub use super::pull_request::Entity as PullRequest;
    pub use super::repo_collaborator::Entity as RepoCollaborator;
    pub use super::repository::Entity as Repository;
    pub use super::ssh_key::Entity as SshKey;
    pub use super::user::Entity as User;
    pub use super::webhook::Entity as Webhook;
    pub use super::workflow_job::Entity as WorkflowJob;
    pub use super::workflow_run::Entity as WorkflowRun;
}

use async_graphql::{ComplexObject, Context, SimpleObject};
use chrono::{DateTime, Utc};
use hiqlite::params;
use uuid::Uuid;

use crate::context::AppContext;

#[derive(SimpleObject, Clone)]
pub struct UserObject {
    pub id: Uuid,
    pub username: String,
    pub email: String,
    pub is_admin: bool,
    pub avatar_url: Option<String>,
    pub created_at: DateTime<Utc>,
    pub deactivated_at: Option<DateTime<Utc>>,
}

impl From<entity::user::Model> for UserObject {
    fn from(m: entity::user::Model) -> Self {
        Self {
            id: m.id,
            username: m.username,
            email: m.email,
            is_admin: m.is_admin,
            avatar_url: m.avatar_url,
            created_at: m.created_at,
            deactivated_at: m.deactivated_at,
        }
    }
}

#[derive(SimpleObject, Clone)]
#[graphql(complex)]
pub struct RepositoryObject {
    pub id: Uuid,
    pub owner_type: String,
    pub owner_id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub is_private: bool,
    pub default_branch: String,
    pub created_at: DateTime<Utc>,
    /// Login (username or org name) of the owner; needed to address the
    /// on-disk git repository. Not exposed to GraphQL clients directly.
    #[graphql(skip)]
    pub owner_login: String,
}

impl RepositoryObject {
    pub async fn from_model(db: &hiqlite::Client, m: entity::repository::Model) -> Self {
        let owner_login = resolve_owner_login(db, &m.owner_type, m.owner_id)
            .await
            .unwrap_or_default();
        Self {
            id: m.id,
            owner_type: m.owner_type,
            owner_id: m.owner_id,
            name: m.name,
            description: m.description,
            is_private: m.is_private,
            default_branch: m.default_branch,
            created_at: m.created_at,
            owner_login,
        }
    }
}

pub async fn resolve_owner_login(db: &hiqlite::Client, owner_type: &str, owner_id: Uuid) -> Option<String> {
    if owner_type == "organization" {
        db.query_as::<entity::organization::Model, _>(
            "SELECT * FROM organizations WHERE id = ?1",
            params!(owner_id.to_string()),
        )
        .await
        .ok()
        .and_then(|v| v.into_iter().next())
        .map(|o| o.name)
    } else {
        db.query_as::<entity::user::Model, _>(
            "SELECT * FROM users WHERE id = ?1",
            params!(owner_id.to_string()),
        )
        .await
        .ok()
        .and_then(|v| v.into_iter().next())
        .map(|u| u.username)
    }
}

#[ComplexObject]
impl RepositoryObject {
    /// Login (username or org name) of the repository owner, resolved
    /// dynamically via the polymorphic owner_type/owner_id pair. Exposed
    /// so clients can build clean `/owner/repo` URLs instead of falling
    /// back to raw owner UUIDs.
    async fn owner_login(&self, ctx: &Context<'_>) -> async_graphql::Result<String> {
        let app = ctx.data::<AppContext>()?;
        resolve_owner_login(&app.db, &self.owner_type, self.owner_id)
            .await
            .ok_or_else(|| {
                async_graphql::Error::new(format!(
                    "dangling owner reference: repository {} has owner_type={:?} owner_id={} with no matching record",
                    self.id, self.owner_type, self.owner_id
                ))
            })
    }

    async fn issues(&self, ctx: &Context<'_>) -> async_graphql::Result<Vec<IssueObject>> {
        let app = ctx.data::<AppContext>()?;
        let issues = app
            .db
            .query_as::<entity::issue::Model, _>(
                "SELECT * FROM issues WHERE repo_id = ?1 ORDER BY created_at DESC",
                params!(self.id.to_string()),
            )
            .await?;
        Ok(issues.into_iter().map(IssueObject::from).collect())
    }

    async fn pull_requests(&self, ctx: &Context<'_>) -> async_graphql::Result<Vec<PullRequestObject>> {
        let app = ctx.data::<AppContext>()?;
        let prs = app
            .db
            .query_as::<entity::pull_request::Model, _>(
                "SELECT * FROM pull_requests WHERE repo_id = ?1 ORDER BY created_at DESC",
                params!(self.id.to_string()),
            )
            .await?;
        Ok(prs.into_iter().map(PullRequestObject::from).collect())
    }

    async fn workflow_runs(&self, ctx: &Context<'_>) -> async_graphql::Result<Vec<WorkflowRunObject>> {
        let app = ctx.data::<AppContext>()?;
        let runs = app
            .db
            .query_as::<entity::workflow_run::Model, _>(
                "SELECT * FROM workflow_runs WHERE repo_id = ?1",
                params!(self.id.to_string()),
            )
            .await?;
        Ok(runs.into_iter().map(WorkflowRunObject::from).collect())
    }

    async fn packages(&self, ctx: &Context<'_>) -> async_graphql::Result<Vec<PackageObject>> {
        let app = ctx.data::<AppContext>()?;
        let packages = app
            .db
            .query_as::<entity::package::Model, _>(
                "SELECT * FROM packages WHERE repo_id = ?1 ORDER BY created_at DESC",
                params!(self.id.to_string()),
            )
            .await?;
        Ok(packages.into_iter().map(PackageObject::from).collect())
    }

    /// Names of the Actions secrets configured for this repository. Values
    /// are never exposed via GraphQL (write-only, standard practice).
    async fn secret_names(&self, ctx: &Context<'_>) -> async_graphql::Result<Vec<String>> {
        let app = ctx.data::<AppContext>()?;
        let secrets = app
            .db
            .query_as::<entity::repo_secret::Model, _>(
                "SELECT * FROM repo_secrets WHERE repo_id = ?1",
                params!(self.id.to_string()),
            )
            .await?;
        Ok(secrets.into_iter().map(|s| s.name).collect())
    }

    async fn branches(&self, ctx: &Context<'_>) -> async_graphql::Result<Vec<BranchObject>> {
        let app = ctx.data::<AppContext>()?;
        let names = app
            .repo_manager
            .list_branches(&self.owner_login, &self.name)
            .map_err(|e| async_graphql::Error::new(e.to_string()))?;
        Ok(names.into_iter().map(|name| BranchObject { name }).collect())
    }

    async fn tree(
        &self,
        ctx: &Context<'_>,
        #[graphql(name = "ref", default = "\"HEAD\".to_string()")] r#ref: String,
        #[graphql(default)] path: String,
    ) -> async_graphql::Result<Vec<TreeEntryObject>> {
        let app = ctx.data::<AppContext>()?;
        let entries = app
            .repo_manager
            .list_tree(&self.owner_login, &self.name, &r#ref, &path)
            .map_err(|e| async_graphql::Error::new(e.to_string()))?;
        Ok(entries.into_iter().map(TreeEntryObject::from).collect())
    }

    async fn wiki_pages(&self, ctx: &Context<'_>) -> async_graphql::Result<Vec<String>> {
        let app = ctx.data::<AppContext>()?;
        let pages = app
            .repo_manager
            .wiki_list_pages(&self.owner_login, &self.name)
            .map_err(|e| async_graphql::Error::new(e.to_string()))?;
        Ok(pages)
    }

    /// The Markdown content of a single wiki page, or `null` if it doesn't exist.
    async fn wiki_page(&self, ctx: &Context<'_>, page: String) -> async_graphql::Result<Option<String>> {
        let app = ctx.data::<AppContext>()?;
        match app.repo_manager.wiki_read_page(&self.owner_login, &self.name, &page) {
            Ok(content) => Ok(Some(content)),
            Err(_) => Ok(None),
        }
    }

    async fn commits(
        &self,
        ctx: &Context<'_>,
        #[graphql(name = "ref", default = "\"HEAD\".to_string()")] r#ref: String,
        #[graphql(default = 20)] limit: i32,
    ) -> async_graphql::Result<Vec<CommitObject>> {
        let app = ctx.data::<AppContext>()?;
        let commits = app
            .repo_manager
            .commit_log(&self.owner_login, &self.name, &r#ref, limit.max(0) as usize)
            .map_err(|e| async_graphql::Error::new(e.to_string()))?;
        Ok(commits.into_iter().map(CommitObject::from).collect())
    }

    async fn branch_protection_rules(
        &self,
        ctx: &Context<'_>,
    ) -> async_graphql::Result<Vec<BranchProtectionRuleObject>> {
        let app = ctx.data::<AppContext>()?;
        let rules = app
            .db
            .query_as::<entity::branch_protection_rule::Model, _>(
                "SELECT * FROM branch_protection_rules WHERE repo_id = ?1",
                params!(self.id.to_string()),
            )
            .await?;
        Ok(rules.into_iter().map(BranchProtectionRuleObject::from).collect())
    }

    /// Recent activity events (pushes, issues, PRs, merges) for this repository.
    async fn activity(
        &self,
        ctx: &Context<'_>,
        #[graphql(default = 20)] limit: i32,
    ) -> async_graphql::Result<Vec<ActivityEventObject>> {
        let app = ctx.data::<AppContext>()?;
        let events = app
            .db
            .query_as::<entity::activity_event::Model, _>(
                "SELECT * FROM activity_events WHERE repo_id = ?1 ORDER BY created_at DESC LIMIT ?2",
                params!(self.id.to_string(), limit.max(0) as i64),
            )
            .await?;
        Ok(events.into_iter().map(ActivityEventObject::from).collect())
    }

    async fn labels(&self, ctx: &Context<'_>) -> async_graphql::Result<Vec<LabelObject>> {
        let app = ctx.data::<AppContext>()?;
        let labels = app
            .db
            .query_as::<entity::label::Model, _>(
                "SELECT * FROM labels WHERE repo_id = ?1",
                params!(self.id.to_string()),
            )
            .await?;
        Ok(labels.into_iter().map(LabelObject::from).collect())
    }

    async fn milestones(&self, ctx: &Context<'_>) -> async_graphql::Result<Vec<MilestoneObject>> {
        let app = ctx.data::<AppContext>()?;
        let milestones = app
            .db
            .query_as::<entity::milestone::Model, _>(
                "SELECT * FROM milestones WHERE repo_id = ?1",
                params!(self.id.to_string()),
            )
            .await?;
        Ok(milestones.into_iter().map(MilestoneObject::from).collect())
    }

    /// Kanban projects for this repository.
    async fn projects(&self, ctx: &Context<'_>) -> async_graphql::Result<Vec<ProjectObject>> {
        let app = ctx.data::<AppContext>()?;
        let projects = app
            .db
            .query_as::<entity::project::Model, _>(
                "SELECT * FROM projects WHERE repo_id = ?1",
                params!(self.id.to_string()),
            )
            .await?;
        Ok(projects.into_iter().map(ProjectObject::from).collect())
    }

    async fn webhooks(&self, ctx: &Context<'_>) -> async_graphql::Result<Vec<WebhookObject>> {
        let app = ctx.data::<AppContext>()?;
        let hooks = app
            .db
            .query_as::<entity::webhook::Model, _>(
                "SELECT * FROM webhooks WHERE repo_id = ?1",
                params!(self.id.to_string()),
            )
            .await?;
        Ok(hooks.into_iter().map(WebhookObject::from).collect())
    }

    /// Users granted explicit collaborator access to this repository.
    async fn collaborators(&self, ctx: &Context<'_>) -> async_graphql::Result<Vec<CollaboratorObject>> {
        let app = ctx.data::<AppContext>()?;
        let collabs = app
            .db
            .query_as::<entity::repo_collaborator::Model, _>(
                "SELECT * FROM repo_collaborators WHERE repo_id = ?1",
                params!(self.id.to_string()),
            )
            .await?;
        let mut out = Vec::with_capacity(collabs.len());
        for c in collabs {
            if let Some(user) = app
                .db
                .query_as::<entity::user::Model, _>(
                    "SELECT * FROM users WHERE id = ?1",
                    params!(c.user_id.to_string()),
                )
                .await?
                .into_iter()
                .next()
            {
                out.push(CollaboratorObject {
                    user: UserObject::from(user),
                    permission: c.permission,
                });
            }
        }
        Ok(out)
    }
}

#[derive(SimpleObject, Clone)]
#[graphql(complex)]
pub struct IssueObject {
    pub id: Uuid,
    pub repo_id: Uuid,
    pub number: i32,
    pub title: String,
    pub body: Option<String>,
    pub author_id: Uuid,
    pub state: String,
    pub created_at: DateTime<Utc>,
    pub closed_at: Option<DateTime<Utc>>,
    #[graphql(skip)]
    pub milestone_id: Option<Uuid>,
}

impl From<entity::issue::Model> for IssueObject {
    fn from(m: entity::issue::Model) -> Self {
        Self {
            id: m.id,
            repo_id: m.repo_id,
            number: m.number,
            title: m.title,
            body: m.body,
            author_id: m.author_id,
            state: m.state,
            created_at: m.created_at,
            closed_at: m.closed_at,
            milestone_id: m.milestone_id,
        }
    }
}

#[ComplexObject]
impl IssueObject {
    async fn labels(&self, ctx: &Context<'_>) -> async_graphql::Result<Vec<LabelObject>> {
        let app = ctx.data::<AppContext>()?;
        let links = app
            .db
            .query_as::<entity::issue_label::Model, _>(
                "SELECT * FROM issue_labels WHERE issue_id = ?1",
                params!(self.id.to_string()),
            )
            .await?;
        let label_ids: Vec<Uuid> = links.into_iter().map(|l| l.label_id).collect();
        if label_ids.is_empty() {
            return Ok(vec![]);
        }
        let placeholders: Vec<String> = (1..=label_ids.len()).map(|i| format!("?{i}")).collect();
        let sql = format!("SELECT * FROM labels WHERE id IN ({})", placeholders.join(", "));
        let params_vec: Vec<hiqlite::Param> = label_ids.iter().map(|id| hiqlite::Param::Text(id.to_string())).collect();
        let labels = app.db.query_as::<entity::label::Model, _>(sql, params_vec).await?;
        Ok(labels.into_iter().map(LabelObject::from).collect())
    }

    async fn milestone(&self, ctx: &Context<'_>) -> async_graphql::Result<Option<MilestoneObject>> {
        let Some(mid) = self.milestone_id else {
            return Ok(None);
        };
        let app = ctx.data::<AppContext>()?;
        let milestone = app
            .db
            .query_as::<entity::milestone::Model, _>(
                "SELECT * FROM milestones WHERE id = ?1",
                params!(mid.to_string()),
            )
            .await?
            .into_iter()
            .next();
        Ok(milestone.map(MilestoneObject::from))
    }
}

#[derive(SimpleObject, Clone)]
pub struct IssueCommentObject {
    pub id: Uuid,
    pub issue_id: Uuid,
    pub author_id: Uuid,
    pub body: String,
    pub created_at: DateTime<Utc>,
}

impl From<entity::issue_comment::Model> for IssueCommentObject {
    fn from(m: entity::issue_comment::Model) -> Self {
        Self {
            id: m.id,
            issue_id: m.issue_id,
            author_id: m.author_id,
            body: m.body,
            created_at: m.created_at,
        }
    }
}

#[derive(SimpleObject, Clone)]
pub struct PullRequestObject {
    pub id: Uuid,
    pub repo_id: Uuid,
    pub number: i32,
    pub title: String,
    pub body: Option<String>,
    pub author_id: Uuid,
    pub source_branch: String,
    pub target_branch: String,
    pub state: String,
    pub created_at: DateTime<Utc>,
    pub merged_at: Option<DateTime<Utc>>,
}

impl From<entity::pull_request::Model> for PullRequestObject {
    fn from(m: entity::pull_request::Model) -> Self {
        Self {
            id: m.id,
            repo_id: m.repo_id,
            number: m.number,
            title: m.title,
            body: m.body,
            author_id: m.author_id,
            source_branch: m.source_branch,
            target_branch: m.target_branch,
            state: m.state,
            created_at: m.created_at,
            merged_at: m.merged_at,
        }
    }
}

#[derive(SimpleObject, Clone)]
pub struct OrganizationObject {
    pub id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub created_at: DateTime<Utc>,
}

impl From<entity::organization::Model> for OrganizationObject {
    fn from(m: entity::organization::Model) -> Self {
        Self {
            id: m.id,
            name: m.name,
            description: m.description,
            created_at: m.created_at,
        }
    }
}

#[derive(SimpleObject, Clone)]
#[graphql(complex)]
pub struct WorkflowRunObject {
    pub id: Uuid,
    pub repo_id: Uuid,
    pub workflow_name: String,
    pub commit_sha: String,
    pub event: String,
    pub status: String,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
}

impl From<entity::workflow_run::Model> for WorkflowRunObject {
    fn from(m: entity::workflow_run::Model) -> Self {
        Self {
            id: m.id,
            repo_id: m.repo_id,
            workflow_name: m.workflow_name,
            commit_sha: m.commit_sha,
            event: m.event,
            status: m.status,
            started_at: m.started_at,
            finished_at: m.finished_at,
        }
    }
}

#[ComplexObject]
impl WorkflowRunObject {
    /// Artifacts uploaded by `genome/upload-artifact` steps across this
    /// run's jobs.
    async fn artifacts(&self, ctx: &Context<'_>) -> async_graphql::Result<Vec<ArtifactObject>> {
        let app = ctx.data::<AppContext>()?;
        let artifacts = app
            .db
            .query_as::<entity::workflow_artifact::Model, _>(
                "SELECT * FROM workflow_artifacts WHERE run_id = ?1",
                params!(self.id.to_string()),
            )
            .await?;
        Ok(artifacts.into_iter().map(ArtifactObject::from).collect())
    }
}

#[derive(SimpleObject, Clone)]
pub struct ArtifactObject {
    pub id: Uuid,
    pub run_id: Uuid,
    pub job_id: Option<Uuid>,
    pub name: String,
    pub size_bytes: i64,
    pub created_at: DateTime<Utc>,
}

impl From<entity::workflow_artifact::Model> for ArtifactObject {
    fn from(m: entity::workflow_artifact::Model) -> Self {
        Self {
            id: m.id,
            run_id: m.run_id,
            job_id: m.job_id,
            name: m.name,
            size_bytes: m.size_bytes,
            created_at: m.created_at,
        }
    }
}

#[derive(SimpleObject, Clone)]
pub struct DevWorkspaceObject {
    pub id: Uuid,
    pub owner_id: Uuid,
    pub repo_id: Option<Uuid>,
    pub name: String,
    pub image: String,
    pub status: String,
    pub container_id: Option<String>,
    pub created_at: DateTime<Utc>,
    pub auto_stop_minutes: Option<i32>,
    pub last_activity_at: Option<DateTime<Utc>>,
    /// Set once a standalone runner has claimed and created this
    /// workspace's container; `null` for workspaces hosted directly by
    /// `server`'s own Docker daemon (the default).
    pub runner_id: Option<String>,
}

impl From<entity::dev_workspace::Model> for DevWorkspaceObject {
    fn from(m: entity::dev_workspace::Model) -> Self {
        Self {
            id: m.id,
            owner_id: m.owner_id,
            repo_id: m.repo_id,
            name: m.name,
            image: m.image,
            status: m.status,
            container_id: m.container_id,
            created_at: m.created_at,
            auto_stop_minutes: m.auto_stop_minutes,
            last_activity_at: m.last_activity_at,
            runner_id: m.runner_id,
        }
    }
}

#[derive(SimpleObject, Clone)]
pub struct BranchObject {
    pub name: String,
}

#[derive(SimpleObject, Clone)]
pub struct CommitObject {
    pub sha: String,
    pub author_name: String,
    pub author_email: String,
    pub message: String,
    pub timestamp: DateTime<Utc>,
}

impl From<git_core::CommitInfo> for CommitObject {
    fn from(c: git_core::CommitInfo) -> Self {
        Self {
            sha: c.sha,
            author_name: c.author_name,
            author_email: c.author_email,
            message: c.message,
            timestamp: c.timestamp,
        }
    }
}

#[derive(SimpleObject, Clone)]
pub struct TreeEntryObject {
    pub name: String,
    pub path: String,
    pub kind: String,
    pub size: Option<i64>,
    pub oid: String,
}

impl From<git_core::TreeEntry> for TreeEntryObject {
    fn from(e: git_core::TreeEntry) -> Self {
        Self {
            name: e.name,
            path: e.path,
            kind: e.kind.as_str().to_string(),
            size: e.size.map(|s| s as i64),
            oid: e.oid,
        }
    }
}

#[derive(SimpleObject, Clone)]
pub struct AuthPayload {
    pub token: String,
    pub user: UserObject,
}

#[derive(SimpleObject, Clone)]
pub struct NotificationObject {
    pub id: Uuid,
    pub user_id: Uuid,
    pub kind: String,
    pub repo_id: Uuid,
    pub subject_id: Uuid,
    pub message: String,
    pub read_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

impl From<entity::notification::Model> for NotificationObject {
    fn from(m: entity::notification::Model) -> Self {
        Self {
            id: m.id,
            user_id: m.user_id,
            kind: m.kind,
            repo_id: m.repo_id,
            subject_id: m.subject_id,
            message: m.message,
            read_at: m.read_at,
            created_at: m.created_at,
        }
    }
}

/// Metadata for a published package. File contents are never exposed via
/// GraphQL; they are served over the dedicated `/packages/:owner/:name/:version`
/// HTTP routes instead.
#[derive(SimpleObject, Clone)]
pub struct PackageObject {
    pub id: Uuid,
    pub repo_id: Option<Uuid>,
    pub name: String,
    pub version: String,
    pub package_type: String,
    pub size_bytes: i64,
    pub created_at: DateTime<Utc>,
}

impl From<entity::package::Model> for PackageObject {
    fn from(m: entity::package::Model) -> Self {
        Self {
            id: m.id,
            repo_id: m.repo_id,
            name: m.name,
            version: m.version,
            package_type: m.package_type,
            size_bytes: m.size_bytes,
            created_at: m.created_at,
        }
    }
}

#[derive(SimpleObject, Clone)]
pub struct ActivityEventObject {
    pub id: Uuid,
    pub repo_id: Option<Uuid>,
    pub actor_id: Uuid,
    pub kind: String,
    pub summary: String,
    pub created_at: DateTime<Utc>,
}

impl From<entity::activity_event::Model> for ActivityEventObject {
    fn from(m: entity::activity_event::Model) -> Self {
        Self {
            id: m.id,
            repo_id: m.repo_id,
            actor_id: m.actor_id,
            kind: m.kind,
            summary: m.summary,
            created_at: m.created_at,
        }
    }
}

/// A single code-search hit: one file, in one repository, that matched.
#[derive(SimpleObject, Clone)]
pub struct CodeSearchResultObject {
    pub repository: RepositoryObject,
    pub path: String,
    /// An FTS5-generated excerpt around the match, with `[b]...[/b]` markers.
    pub snippet: String,
}

/// Result bundle for the basic `search` query, grouping matches by entity kind.
#[derive(SimpleObject, Clone)]
pub struct SearchResults {
    pub repositories: Vec<RepositoryObject>,
    pub issues: Vec<IssueObject>,
    pub users: Vec<UserObject>,
    pub code: Vec<CodeSearchResultObject>,
}

#[derive(SimpleObject, Clone)]
pub struct BranchProtectionRuleObject {
    pub id: Uuid,
    pub repo_id: Uuid,
    pub branch_pattern: String,
    pub require_reviews_count: i32,
    pub require_status_checks: bool,
    pub block_force_push: bool,
    pub created_at: DateTime<Utc>,
}

impl From<entity::branch_protection_rule::Model> for BranchProtectionRuleObject {
    fn from(m: entity::branch_protection_rule::Model) -> Self {
        Self {
            id: m.id,
            repo_id: m.repo_id,
            branch_pattern: m.branch_pattern,
            require_reviews_count: m.require_reviews_count,
            require_status_checks: m.require_status_checks,
            block_force_push: m.block_force_push,
            created_at: m.created_at,
        }
    }
}

#[derive(SimpleObject, Clone)]
#[graphql(complex)]
pub struct PrReviewObject {
    pub id: Uuid,
    pub pr_id: Uuid,
    pub reviewer_id: Uuid,
    pub state: String,
    pub body: Option<String>,
    pub created_at: DateTime<Utc>,
}

impl From<entity::pr_review::Model> for PrReviewObject {
    fn from(m: entity::pr_review::Model) -> Self {
        Self {
            id: m.id,
            pr_id: m.pr_id,
            reviewer_id: m.reviewer_id,
            state: m.state,
            body: m.body,
            created_at: m.created_at,
        }
    }
}

#[ComplexObject]
impl PrReviewObject {
    /// Inline comments left on specific lines of the diff as part of this review.
    async fn comments(&self, ctx: &Context<'_>) -> async_graphql::Result<Vec<PrReviewCommentObject>> {
        let app = ctx.data::<AppContext>()?;
        let comments = app
            .db
            .query_as::<entity::pr_review_comment::Model, _>(
                "SELECT * FROM pr_review_comments WHERE review_id = ?1 ORDER BY created_at ASC",
                params!(self.id.to_string()),
            )
            .await?;
        Ok(comments.into_iter().map(PrReviewCommentObject::from).collect())
    }
}

#[derive(SimpleObject, Clone)]
pub struct PrReviewCommentObject {
    pub id: Uuid,
    pub review_id: Uuid,
    pub file_path: String,
    pub line_number: i32,
    pub body: String,
    pub created_at: DateTime<Utc>,
}

impl From<entity::pr_review_comment::Model> for PrReviewCommentObject {
    fn from(m: entity::pr_review_comment::Model) -> Self {
        Self {
            id: m.id,
            review_id: m.review_id,
            file_path: m.file_path,
            line_number: m.line_number,
            body: m.body,
            created_at: m.created_at,
        }
    }
}

#[derive(SimpleObject, Clone)]
pub struct LabelObject {
    pub id: Uuid,
    pub repo_id: Uuid,
    pub name: String,
    pub color: String,
}

impl From<entity::label::Model> for LabelObject {
    fn from(m: entity::label::Model) -> Self {
        Self {
            id: m.id,
            repo_id: m.repo_id,
            name: m.name,
            color: m.color,
        }
    }
}

#[derive(SimpleObject, Clone)]
pub struct MilestoneObject {
    pub id: Uuid,
    pub repo_id: Uuid,
    pub title: String,
    pub description: Option<String>,
    pub due_date: Option<DateTime<Utc>>,
    pub state: String,
    pub created_at: DateTime<Utc>,
}

impl From<entity::milestone::Model> for MilestoneObject {
    fn from(m: entity::milestone::Model) -> Self {
        Self {
            id: m.id,
            repo_id: m.repo_id,
            title: m.title,
            description: m.description,
            due_date: m.due_date,
            state: m.state,
            created_at: m.created_at,
        }
    }
}

#[derive(SimpleObject, Clone)]
#[graphql(complex)]
pub struct ProjectObject {
    pub id: Uuid,
    pub repo_id: Uuid,
    pub name: String,
    pub created_at: DateTime<Utc>,
}

impl From<entity::project::Model> for ProjectObject {
    fn from(m: entity::project::Model) -> Self {
        Self {
            id: m.id,
            repo_id: m.repo_id,
            name: m.name,
            created_at: m.created_at,
        }
    }
}

#[ComplexObject]
impl ProjectObject {
    /// Columns belonging to this project, in display order.
    async fn columns(&self, ctx: &Context<'_>) -> async_graphql::Result<Vec<ProjectColumnObject>> {
        let app = ctx.data::<AppContext>()?;
        let columns = app
            .db
            .query_as::<entity::project_column::Model, _>(
                "SELECT * FROM project_columns WHERE project_id = ?1 ORDER BY position ASC",
                params!(self.id.to_string()),
            )
            .await?;
        Ok(columns.into_iter().map(ProjectColumnObject::from).collect())
    }
}

#[derive(SimpleObject, Clone)]
#[graphql(complex)]
pub struct ProjectColumnObject {
    pub id: Uuid,
    pub project_id: Uuid,
    pub name: String,
    pub position: i32,
}

impl From<entity::project_column::Model> for ProjectColumnObject {
    fn from(m: entity::project_column::Model) -> Self {
        Self {
            id: m.id,
            project_id: m.project_id,
            name: m.name,
            position: m.position,
        }
    }
}

#[ComplexObject]
impl ProjectColumnObject {
    /// Cards placed in this column, in display order.
    async fn cards(&self, ctx: &Context<'_>) -> async_graphql::Result<Vec<ProjectCardObject>> {
        let app = ctx.data::<AppContext>()?;
        let cards = app
            .db
            .query_as::<entity::project_card::Model, _>(
                "SELECT * FROM project_cards WHERE column_id = ?1 ORDER BY position ASC",
                params!(self.id.to_string()),
            )
            .await?;
        Ok(cards.into_iter().map(ProjectCardObject::from).collect())
    }
}

#[derive(SimpleObject, Clone)]
#[graphql(complex)]
pub struct ProjectCardObject {
    pub id: Uuid,
    pub column_id: Uuid,
    pub issue_id: Option<Uuid>,
    pub pull_request_id: Option<Uuid>,
    pub position: i32,
}

impl From<entity::project_card::Model> for ProjectCardObject {
    fn from(m: entity::project_card::Model) -> Self {
        Self {
            id: m.id,
            column_id: m.column_id,
            issue_id: m.issue_id,
            pull_request_id: m.pull_request_id,
            position: m.position,
        }
    }
}

#[ComplexObject]
impl ProjectCardObject {
    /// The issue this card represents, if it wraps an issue rather than a PR.
    async fn issue(&self, ctx: &Context<'_>) -> async_graphql::Result<Option<IssueObject>> {
        let Some(iid) = self.issue_id else {
            return Ok(None);
        };
        let app = ctx.data::<AppContext>()?;
        let issue = app
            .db
            .query_as::<entity::issue::Model, _>("SELECT * FROM issues WHERE id = ?1", params!(iid.to_string()))
            .await?
            .into_iter()
            .next();
        Ok(issue.map(IssueObject::from))
    }
}

/// A personal access token (PAT), usable as `Authorization: token <value>`
/// against the GraphQL API, git smart-HTTP, and REST routes. The plaintext
/// token itself is never stored (only its SHA256 hash) and is only ever
/// exposed once, from `createAccessToken`.
#[derive(SimpleObject, Clone)]
pub struct AccessTokenObject {
    pub id: Uuid,
    pub name: String,
    pub scopes: Vec<String>,
    pub expires_at: Option<DateTime<Utc>>,
}

impl From<entity::access_token::Model> for AccessTokenObject {
    fn from(m: entity::access_token::Model) -> Self {
        let scopes = serde_json::from_str(&m.scopes).unwrap_or_default();
        Self {
            id: m.id,
            name: m.name,
            scopes,
            expires_at: m.expires_at,
        }
    }
}

/// Returned once, at creation time, from `createAccessToken`. The plaintext
/// `token` is never stored or retrievable again — only its hash is persisted.
#[derive(SimpleObject, Clone)]
pub struct AccessTokenCreated {
    pub token: String,
    pub access_token: AccessTokenObject,
}

#[derive(SimpleObject, Clone)]
pub struct WebhookObject {
    pub id: Uuid,
    pub repo_id: Uuid,
    pub target_url: String,
    pub events: Vec<String>,
    pub active: bool,
}

impl From<entity::webhook::Model> for WebhookObject {
    fn from(m: entity::webhook::Model) -> Self {
        let events = serde_json::from_str(&m.events).unwrap_or_default();
        Self {
            id: m.id,
            repo_id: m.repo_id,
            target_url: m.target_url,
            events,
            active: m.active,
        }
    }
}

/// A repository collaborator: a user granted explicit access alongside
/// (or instead of) organization/ownership-derived permission.
#[derive(SimpleObject, Clone)]
pub struct CollaboratorObject {
    pub user: UserObject,
    pub permission: String,
}

#[derive(SimpleObject, Clone)]
pub struct SshKeyObject {
    pub id: Uuid,
    pub title: String,
    pub fingerprint: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl From<entity::ssh_key::Model> for SshKeyObject {
    fn from(m: entity::ssh_key::Model) -> Self {
        Self {
            id: m.id,
            title: m.title,
            fingerprint: m.fingerprint,
            created_at: m.created_at,
        }
    }
}

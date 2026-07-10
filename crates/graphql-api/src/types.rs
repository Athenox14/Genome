use async_graphql::{ComplexObject, Context, SimpleObject};
use chrono::{DateTime, Utc};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder};
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
    pub async fn from_model(
        db: &sea_orm::DatabaseConnection,
        m: entity::repository::Model,
    ) -> Self {
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

pub async fn resolve_owner_login(
    db: &sea_orm::DatabaseConnection,
    owner_type: &str,
    owner_id: Uuid,
) -> Option<String> {
    if owner_type == "organization" {
        entity::prelude::Organization::find_by_id(owner_id)
            .one(db)
            .await
            .ok()
            .flatten()
            .map(|o| o.name)
    } else {
        entity::prelude::User::find_by_id(owner_id)
            .one(db)
            .await
            .ok()
            .flatten()
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
        let issues = entity::prelude::Issue::find()
            .filter(entity::issue::Column::RepoId.eq(self.id))
            .order_by_desc(entity::issue::Column::CreatedAt)
            .all(&app.db)
            .await?;
        Ok(issues.into_iter().map(IssueObject::from).collect())
    }

    async fn pull_requests(&self, ctx: &Context<'_>) -> async_graphql::Result<Vec<PullRequestObject>> {
        let app = ctx.data::<AppContext>()?;
        let prs = entity::prelude::PullRequest::find()
            .filter(entity::pull_request::Column::RepoId.eq(self.id))
            .order_by_desc(entity::pull_request::Column::CreatedAt)
            .all(&app.db)
            .await?;
        Ok(prs.into_iter().map(PullRequestObject::from).collect())
    }

    async fn workflow_runs(&self, ctx: &Context<'_>) -> async_graphql::Result<Vec<WorkflowRunObject>> {
        let app = ctx.data::<AppContext>()?;
        let runs = entity::prelude::WorkflowRun::find()
            .filter(entity::workflow_run::Column::RepoId.eq(self.id))
            .all(&app.db)
            .await?;
        Ok(runs.into_iter().map(WorkflowRunObject::from).collect())
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
}

#[derive(SimpleObject, Clone)]
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
        }
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

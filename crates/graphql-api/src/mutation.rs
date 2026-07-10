use async_graphql::{Context, Object};
use auth::{Claims, ClaimsInput, Permission};
use chrono::Utc;
use regex::Regex;
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, Set};
use std::sync::OnceLock;
use uuid::Uuid;

use crate::context::{AppContext, RequestContext};
use crate::types::{
    resolve_owner_login, AuthPayload, DevWorkspaceObject, IssueCommentObject, IssueObject,
    OrganizationObject, PullRequestObject, RepositoryObject, UserObject, WorkflowRunObject,
};

pub struct MutationRoot;

fn username_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^[a-zA-Z0-9_-]{3,32}$").unwrap())
}

fn email_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^[^@\s]+@[^@\s]+\.[^@\s]+$").unwrap())
}

/// Validates a registration username: safe to use as a directory name via
/// git-core (no path separators, no leading dot, restricted character set).
fn validate_username(username: &str) -> async_graphql::Result<()> {
    if !username_regex().is_match(username) {
        return Err(async_graphql::Error::new(
            "username must be 3-32 characters and contain only letters, digits, '_' or '-'",
        ));
    }
    Ok(())
}

fn validate_email(email: &str) -> async_graphql::Result<()> {
    if !email.contains('@') || !email_regex().is_match(email) {
        return Err(async_graphql::Error::new("invalid email address"));
    }
    Ok(())
}

fn validate_password(password: &str) -> async_graphql::Result<()> {
    if password.len() < 8 {
        return Err(async_graphql::Error::new(
            "password must be at least 8 characters",
        ));
    }
    Ok(())
}

/// Validates a name used as a filesystem path component (repository or
/// organization name), mirroring `git_core::validate_slug` but surfaced as a
/// clear GraphQL error instead of a lower-level `GitCoreError`.
fn validate_name_slug(name: &str) -> async_graphql::Result<()> {
    git_core::validate_slug(name)
        .map_err(|_| async_graphql::Error::new(format!("invalid name: '{name}'")))
}

fn require_user<'a>(req: &'a RequestContext) -> async_graphql::Result<&'a Claims> {
    req.user
        .as_ref()
        .ok_or_else(|| async_graphql::Error::new("unauthenticated"))
}

/// Determine the effective permission the given user has on a repository.
async fn repo_permission(
    app: &AppContext,
    repo: &entity::repository::Model,
    user_id: Uuid,
) -> async_graphql::Result<Option<Permission>> {
    let is_owner = repo.owner_type == "user" && repo.owner_id == user_id;

    let is_admin_org_role = if repo.owner_type == "organization" {
        entity::prelude::OrgMember::find()
            .filter(entity::org_member::Column::OrgId.eq(repo.owner_id))
            .filter(entity::org_member::Column::UserId.eq(user_id))
            .one(&app.db)
            .await?
            .map(|m| m.role == entity::org_member::role::OWNER || m.role == entity::org_member::role::ADMIN)
            .unwrap_or(false)
    } else {
        false
    };

    let collaborator_perm = entity::prelude::RepoCollaborator::find()
        .filter(entity::repo_collaborator::Column::RepoId.eq(repo.id))
        .filter(entity::repo_collaborator::Column::UserId.eq(user_id))
        .one(&app.db)
        .await?
        .map(|c| match c.permission.as_str() {
            entity::repo_collaborator::permission::ADMIN => Permission::Admin,
            entity::repo_collaborator::permission::WRITE => Permission::Write,
            _ => Permission::Read,
        });

    Ok(auth::effective_permission(
        is_owner,
        is_admin_org_role,
        collaborator_perm,
        repo.is_private,
    ))
}

async fn find_repo(app: &AppContext, repo_id: Uuid) -> async_graphql::Result<entity::repository::Model> {
    entity::prelude::Repository::find_by_id(repo_id)
        .one(&app.db)
        .await?
        .ok_or_else(|| async_graphql::Error::new("repository not found"))
}

#[Object]
impl MutationRoot {
    async fn register(
        &self,
        ctx: &Context<'_>,
        username: String,
        email: String,
        password: String,
    ) -> async_graphql::Result<UserObject> {
        validate_username(&username)?;
        validate_email(&email)?;
        validate_password(&password)?;

        let app = ctx.data::<AppContext>()?;
        let password_hash = auth::hash_password(&password)
            .map_err(|e| async_graphql::Error::new(e.to_string()))?;

        let user = entity::user::ActiveModel {
            id: Set(Uuid::new_v4()),
            username: Set(username),
            email: Set(email),
            password_hash: Set(password_hash),
            is_admin: Set(false),
            avatar_url: Set(None),
            created_at: Set(Utc::now()),
        };

        let user = user.insert(&app.db).await?;
        Ok(UserObject::from(user))
    }

    async fn login(
        &self,
        ctx: &Context<'_>,
        username: String,
        password: String,
    ) -> async_graphql::Result<AuthPayload> {
        let app = ctx.data::<AppContext>()?;
        let user = entity::prelude::User::find()
            .filter(entity::user::Column::Username.eq(username))
            .one(&app.db)
            .await?
            .ok_or_else(|| async_graphql::Error::new("invalid username or password"))?;

        let valid = auth::verify_password(&password, &user.password_hash)
            .map_err(|e| async_graphql::Error::new(e.to_string()))?;
        if !valid {
            return Err(async_graphql::Error::new("invalid username or password"));
        }

        let token = auth::create_jwt(ClaimsInput::from(&user), &app.jwt_secret, 24 * 7)
            .map_err(|e| async_graphql::Error::new(e.to_string()))?;

        Ok(AuthPayload {
            token,
            user: UserObject::from(user),
        })
    }

    async fn create_repository(
        &self,
        ctx: &Context<'_>,
        name: String,
        description: Option<String>,
        is_private: bool,
    ) -> async_graphql::Result<RepositoryObject> {
        validate_name_slug(&name)?;

        let app = ctx.data::<AppContext>()?;
        let req = ctx.data::<RequestContext>()?;
        let claims = require_user(req)?;

        let owner = entity::prelude::User::find_by_id(claims.sub)
            .one(&app.db)
            .await?
            .ok_or_else(|| async_graphql::Error::new("owner not found"))?;

        app.repo_manager
            .init_repo(&owner.username, &name)
            .map_err(|e| async_graphql::Error::new(e.to_string()))?;

        let repo = entity::repository::ActiveModel {
            id: Set(Uuid::new_v4()),
            owner_type: Set("user".to_string()),
            owner_id: Set(owner.id),
            name: Set(name),
            description: Set(description),
            is_private: Set(is_private),
            default_branch: Set("main".to_string()),
            created_at: Set(Utc::now()),
        };
        let repo = repo.insert(&app.db).await?;

        Ok(RepositoryObject::from_model(&app.db, repo).await)
    }

    async fn delete_repository(&self, ctx: &Context<'_>, repo_id: Uuid) -> async_graphql::Result<bool> {
        let app = ctx.data::<AppContext>()?;
        let req = ctx.data::<RequestContext>()?;
        let claims = require_user(req)?;

        let repo = find_repo(app, repo_id).await?;
        let perm = repo_permission(app, &repo, claims.sub).await?;
        if perm != Some(Permission::Admin) {
            return Err(async_graphql::Error::new("forbidden"));
        }

        let owner_login = resolve_owner_login(&app.db, &repo.owner_type, repo.owner_id)
            .await
            .unwrap_or_default();
        let _ = app.repo_manager.delete_repo(&owner_login, &repo.name);

        entity::prelude::Repository::delete_by_id(repo.id)
            .exec(&app.db)
            .await?;

        Ok(true)
    }

    async fn create_issue(
        &self,
        ctx: &Context<'_>,
        repo_id: Uuid,
        title: String,
        body: Option<String>,
    ) -> async_graphql::Result<IssueObject> {
        let app = ctx.data::<AppContext>()?;
        let req = ctx.data::<RequestContext>()?;
        let claims = require_user(req)?;

        let repo = find_repo(app, repo_id).await?;
        let perm = repo_permission(app, &repo, claims.sub).await?;
        if perm.is_none() {
            return Err(async_graphql::Error::new("forbidden"));
        }

        let existing_count = entity::prelude::Issue::find()
            .filter(entity::issue::Column::RepoId.eq(repo_id))
            .all(&app.db)
            .await?
            .len();

        let issue = entity::issue::ActiveModel {
            id: Set(Uuid::new_v4()),
            repo_id: Set(repo_id),
            number: Set(existing_count as i32 + 1),
            title: Set(title),
            body: Set(body),
            author_id: Set(claims.sub),
            state: Set(entity::issue::state::OPEN.to_string()),
            created_at: Set(Utc::now()),
            closed_at: Set(None),
        };
        let issue = issue.insert(&app.db).await?;

        let payload = serde_json::json!({
            "action": "opened",
            "issue": {
                "id": issue.id,
                "number": issue.number,
                "title": issue.title,
                "body": issue.body,
                "state": issue.state,
            },
            "repository": { "id": repo.id, "name": repo.name },
        });
        let _ = app.webhook_dispatcher.dispatch(repo_id, "issues", payload).await;

        Ok(IssueObject::from(issue))
    }

    async fn comment_on_issue(
        &self,
        ctx: &Context<'_>,
        issue_id: Uuid,
        body: String,
    ) -> async_graphql::Result<IssueCommentObject> {
        let app = ctx.data::<AppContext>()?;
        let req = ctx.data::<RequestContext>()?;
        let claims = require_user(req)?;

        let issue = entity::prelude::Issue::find_by_id(issue_id)
            .one(&app.db)
            .await?
            .ok_or_else(|| async_graphql::Error::new("issue not found"))?;
        let repo = find_repo(app, issue.repo_id).await?;
        let perm = repo_permission(app, &repo, claims.sub).await?;
        if perm.is_none() {
            return Err(async_graphql::Error::new("forbidden"));
        }

        let comment = entity::issue_comment::ActiveModel {
            id: Set(Uuid::new_v4()),
            issue_id: Set(issue_id),
            author_id: Set(claims.sub),
            body: Set(body),
            created_at: Set(Utc::now()),
        };
        let comment = comment.insert(&app.db).await?;

        let payload = serde_json::json!({
            "action": "created",
            "issue": { "id": issue.id, "number": issue.number, "title": issue.title },
            "comment": { "id": comment.id, "body": comment.body },
            "repository": { "id": repo.id, "name": repo.name },
        });
        let _ = app
            .webhook_dispatcher
            .dispatch(repo.id, "issue_comment", payload)
            .await;

        Ok(IssueCommentObject::from(comment))
    }

    async fn create_pull_request(
        &self,
        ctx: &Context<'_>,
        repo_id: Uuid,
        title: String,
        body: Option<String>,
        source_branch: String,
        target_branch: String,
    ) -> async_graphql::Result<PullRequestObject> {
        let app = ctx.data::<AppContext>()?;
        let req = ctx.data::<RequestContext>()?;
        let claims = require_user(req)?;

        let repo = find_repo(app, repo_id).await?;
        let perm = repo_permission(app, &repo, claims.sub).await?;
        if perm.is_none() {
            return Err(async_graphql::Error::new("forbidden"));
        }

        let existing_count = entity::prelude::PullRequest::find()
            .filter(entity::pull_request::Column::RepoId.eq(repo_id))
            .all(&app.db)
            .await?
            .len();

        let pr = entity::pull_request::ActiveModel {
            id: Set(Uuid::new_v4()),
            repo_id: Set(repo_id),
            number: Set(existing_count as i32 + 1),
            title: Set(title),
            body: Set(body),
            author_id: Set(claims.sub),
            source_branch: Set(source_branch),
            target_branch: Set(target_branch),
            state: Set(entity::pull_request::state::OPEN.to_string()),
            created_at: Set(Utc::now()),
            merged_at: Set(None),
        };
        let pr = pr.insert(&app.db).await?;

        let payload = serde_json::json!({
            "action": "opened",
            "pull_request": {
                "id": pr.id,
                "number": pr.number,
                "title": pr.title,
                "body": pr.body,
                "state": pr.state,
                "source_branch": pr.source_branch,
                "target_branch": pr.target_branch,
            },
            "repository": { "id": repo.id, "name": repo.name },
        });
        let _ = app
            .webhook_dispatcher
            .dispatch(repo_id, "pull_request", payload)
            .await;

        Ok(PullRequestObject::from(pr))
    }

    /// Merges a pull request: performs an actual git merge (fast-forward or
    /// merge commit) of `source_branch` into `target_branch` via git-core,
    /// then marks the PR as merged in the database.
    async fn merge_pull_request(&self, ctx: &Context<'_>, pr_id: Uuid) -> async_graphql::Result<PullRequestObject> {
        let app = ctx.data::<AppContext>()?;
        let req = ctx.data::<RequestContext>()?;
        let claims = require_user(req)?;

        let pr = entity::prelude::PullRequest::find_by_id(pr_id)
            .one(&app.db)
            .await?
            .ok_or_else(|| async_graphql::Error::new("pull request not found"))?;
        let repo = find_repo(app, pr.repo_id).await?;
        let perm = repo_permission(app, &repo, claims.sub).await?;
        if perm < Some(Permission::Write) {
            return Err(async_graphql::Error::new("forbidden"));
        }

        let owner_login = resolve_owner_login(&app.db, &repo.owner_type, repo.owner_id)
            .await
            .unwrap_or_default();

        let message = format!(
            "Merge pull request #{} from {}",
            pr.number, pr.source_branch
        );

        let author_email = format!("{}@users.noreply.local", claims.username);

        match app.repo_manager.merge_branches(
            &owner_login,
            &repo.name,
            &pr.source_branch,
            &pr.target_branch,
            &claims.username,
            &author_email,
            &message,
        ) {
            Ok(_) => {}
            Err(git_core::GitCoreError::MergeConflict(_, _)) => {
                return Err(async_graphql::Error::new(
                    "merge conflict, cannot merge automatically",
                ));
            }
            Err(e) => return Err(async_graphql::Error::new(e.to_string())),
        }

        let mut active: entity::pull_request::ActiveModel = pr.into();
        active.state = Set(entity::pull_request::state::MERGED.to_string());
        active.merged_at = Set(Some(Utc::now()));
        let pr = active.update(&app.db).await?;

        let payload = serde_json::json!({
            "action": "closed",
            "pull_request": {
                "id": pr.id,
                "number": pr.number,
                "title": pr.title,
                "state": pr.state,
                "merged": true,
            },
            "repository": { "id": repo.id, "name": repo.name },
        });
        let _ = app
            .webhook_dispatcher
            .dispatch(repo.id, "pull_request", payload)
            .await;

        Ok(PullRequestObject::from(pr))
    }

    async fn create_organization(
        &self,
        ctx: &Context<'_>,
        name: String,
        description: Option<String>,
    ) -> async_graphql::Result<OrganizationObject> {
        validate_name_slug(&name)?;

        let app = ctx.data::<AppContext>()?;
        let req = ctx.data::<RequestContext>()?;
        let claims = require_user(req)?;

        let org = entity::organization::ActiveModel {
            id: Set(Uuid::new_v4()),
            name: Set(name),
            description: Set(description),
            created_at: Set(Utc::now()),
        };
        let org = org.insert(&app.db).await?;

        let member = entity::org_member::ActiveModel {
            org_id: Set(org.id),
            user_id: Set(claims.sub),
            role: Set(entity::org_member::role::OWNER.to_string()),
        };
        member.insert(&app.db).await?;

        Ok(OrganizationObject::from(org))
    }

    async fn add_collaborator(
        &self,
        ctx: &Context<'_>,
        repo_id: Uuid,
        username: String,
        permission: String,
    ) -> async_graphql::Result<bool> {
        let app = ctx.data::<AppContext>()?;
        let req = ctx.data::<RequestContext>()?;
        let claims = require_user(req)?;

        let repo = find_repo(app, repo_id).await?;
        let perm = repo_permission(app, &repo, claims.sub).await?;
        if perm != Some(Permission::Admin) {
            return Err(async_graphql::Error::new("forbidden"));
        }

        let collaborator = entity::prelude::User::find()
            .filter(entity::user::Column::Username.eq(username))
            .one(&app.db)
            .await?
            .ok_or_else(|| async_graphql::Error::new("user not found"))?;

        let collab = entity::repo_collaborator::ActiveModel {
            repo_id: Set(repo_id),
            user_id: Set(collaborator.id),
            permission: Set(permission),
        };
        collab.insert(&app.db).await?;

        Ok(true)
    }

    /// Loads a workflow definition from the repository's default branch,
    /// parses it, and spawns its execution via the actions Executor,
    /// recording a `workflow_run` row.
    async fn trigger_workflow_dispatch(
        &self,
        ctx: &Context<'_>,
        repo_id: Uuid,
        workflow_path: String,
    ) -> async_graphql::Result<WorkflowRunObject> {
        let app = ctx.data::<AppContext>()?;
        let req = ctx.data::<RequestContext>()?;
        let claims = require_user(req)?;

        let repo = find_repo(app, repo_id).await?;
        let perm = repo_permission(app, &repo, claims.sub).await?;
        if perm < Some(Permission::Write) {
            return Err(async_graphql::Error::new("forbidden"));
        }

        let owner_login = resolve_owner_login(&app.db, &repo.owner_type, repo.owner_id)
            .await
            .unwrap_or_default();

        let default_branch = app
            .repo_manager
            .get_default_branch(&owner_login, &repo.name)
            .map_err(|e| async_graphql::Error::new(e.to_string()))?;

        let contents = app
            .repo_manager
            .read_file_at_ref(&owner_login, &repo.name, &default_branch, &workflow_path)
            .map_err(|e| async_graphql::Error::new(e.to_string()))?;
        let text = String::from_utf8(contents)
            .map_err(|e| async_graphql::Error::new(format!("workflow file is not valid UTF-8: {e}")))?;

        let workflow = actions::Workflow::parse(&text)
            .map_err(|e| async_graphql::Error::new(e.to_string()))?;

        let commits = app
            .repo_manager
            .commit_log(&owner_login, &repo.name, &default_branch, 1)
            .map_err(|e| async_graphql::Error::new(e.to_string()))?;
        let commit_sha = commits
            .first()
            .map(|c| c.sha.clone())
            .unwrap_or_else(|| "unknown".to_string());

        let run = entity::workflow_run::ActiveModel {
            id: Set(Uuid::new_v4()),
            repo_id: Set(repo_id),
            workflow_name: Set(workflow.name.clone().unwrap_or_else(|| workflow_path.clone())),
            commit_sha: Set(commit_sha),
            event: Set("workflow_dispatch".to_string()),
            status: Set(entity::workflow_run::status::QUEUED.to_string()),
            started_at: Set(None),
            finished_at: Set(None),
        };
        let run = run.insert(&app.db).await?;

        // Spawn execution of every job in the background; this is fire-and-forget
        // since GraphQL mutations should return promptly. Job/run status updates
        // would normally be persisted by a background task watching JobResults.
        let executor = app.actions_executor.clone();
        let jobs: Vec<_> = workflow.jobs.into_values().collect();
        tokio::spawn(async move {
            for job in jobs {
                let result = executor
                    .run_job(&job, &[], Default::default(), |line| {
                        tracing::info!(target: "workflow", "{line}");
                    })
                    .await;
                if let Err(e) = result {
                    tracing::warn!("workflow job failed: {e}");
                }
            }
        });

        Ok(WorkflowRunObject::from(run))
    }

    async fn create_dev_workspace(
        &self,
        ctx: &Context<'_>,
        name: String,
        image: Option<String>,
        repo_id: Option<Uuid>,
    ) -> async_graphql::Result<DevWorkspaceObject> {
        let app = ctx.data::<AppContext>()?;
        let req = ctx.data::<RequestContext>()?;
        let claims = require_user(req)?;

        let image = image.unwrap_or_else(|| dev_env::manager::IMAGE_CODE_SERVER.to_string());

        let repo_clone_url = if let Some(rid) = repo_id {
            let repo = find_repo(app, rid).await?;
            let owner_login = resolve_owner_login(&app.db, &repo.owner_type, repo.owner_id)
                .await
                .unwrap_or_default();
            app.repo_manager
                .repo_path(&owner_login, &repo.name)
                .ok()
                .map(|p| p.to_string_lossy().to_string())
        } else {
            None
        };

        let handle = app
            .workspace_manager
            .create_workspace(
                &name,
                &image,
                repo_clone_url.as_deref(),
                None,
                None,
                &claims.username,
            )
            .await
            .map_err(|e| async_graphql::Error::new(e.to_string()))?;

        let workspace = entity::dev_workspace::ActiveModel {
            id: Set(Uuid::new_v4()),
            owner_id: Set(claims.sub),
            repo_id: Set(repo_id),
            name: Set(name),
            image: Set(image),
            status: Set(entity::dev_workspace::status::RUNNING.to_string()),
            container_id: Set(Some(handle.container_id)),
            created_at: Set(Utc::now()),
        };
        let workspace = workspace.insert(&app.db).await?;
        Ok(DevWorkspaceObject::from(workspace))
    }

    async fn start_dev_workspace(&self, ctx: &Context<'_>, workspace_id: Uuid) -> async_graphql::Result<DevWorkspaceObject> {
        let app = ctx.data::<AppContext>()?;
        let req = ctx.data::<RequestContext>()?;
        let claims = require_user(req)?;

        let workspace = entity::prelude::DevWorkspace::find_by_id(workspace_id)
            .one(&app.db)
            .await?
            .ok_or_else(|| async_graphql::Error::new("workspace not found"))?;
        if workspace.owner_id != claims.sub {
            return Err(async_graphql::Error::new("forbidden"));
        }
        let container_id = workspace
            .container_id
            .clone()
            .ok_or_else(|| async_graphql::Error::new("workspace has no container"))?;

        app.workspace_manager
            .start_workspace(&container_id)
            .await
            .map_err(|e| async_graphql::Error::new(e.to_string()))?;

        let mut active: entity::dev_workspace::ActiveModel = workspace.into();
        active.status = Set(entity::dev_workspace::status::RUNNING.to_string());
        let workspace = active.update(&app.db).await?;
        Ok(DevWorkspaceObject::from(workspace))
    }

    async fn stop_dev_workspace(&self, ctx: &Context<'_>, workspace_id: Uuid) -> async_graphql::Result<DevWorkspaceObject> {
        let app = ctx.data::<AppContext>()?;
        let req = ctx.data::<RequestContext>()?;
        let claims = require_user(req)?;

        let workspace = entity::prelude::DevWorkspace::find_by_id(workspace_id)
            .one(&app.db)
            .await?
            .ok_or_else(|| async_graphql::Error::new("workspace not found"))?;
        if workspace.owner_id != claims.sub {
            return Err(async_graphql::Error::new("forbidden"));
        }
        let container_id = workspace
            .container_id
            .clone()
            .ok_or_else(|| async_graphql::Error::new("workspace has no container"))?;

        app.workspace_manager
            .stop_workspace(&container_id)
            .await
            .map_err(|e| async_graphql::Error::new(e.to_string()))?;

        let mut active: entity::dev_workspace::ActiveModel = workspace.into();
        active.status = Set(entity::dev_workspace::status::STOPPED.to_string());
        let workspace = active.update(&app.db).await?;
        Ok(DevWorkspaceObject::from(workspace))
    }

    async fn delete_dev_workspace(&self, ctx: &Context<'_>, workspace_id: Uuid) -> async_graphql::Result<bool> {
        let app = ctx.data::<AppContext>()?;
        let req = ctx.data::<RequestContext>()?;
        let claims = require_user(req)?;

        let workspace = entity::prelude::DevWorkspace::find_by_id(workspace_id)
            .one(&app.db)
            .await?
            .ok_or_else(|| async_graphql::Error::new("workspace not found"))?;
        if workspace.owner_id != claims.sub {
            return Err(async_graphql::Error::new("forbidden"));
        }

        if let Some(container_id) = &workspace.container_id {
            app.workspace_manager
                .delete_workspace(container_id)
                .await
                .map_err(|e| async_graphql::Error::new(e.to_string()))?;
        }

        entity::prelude::DevWorkspace::delete_by_id(workspace.id)
            .exec(&app.db)
            .await?;
        Ok(true)
    }
}

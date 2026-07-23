use async_graphql::{Context, Object};
use auth::{Claims, ClaimsInput, Permission};
use chrono::Utc;
use hiqlite::params;
use regex::Regex;
use std::sync::OnceLock;
use uuid::Uuid;

use crate::context::{AppContext, RequestContext};
use crate::types::{
    resolve_owner_login, AccessTokenCreated, AccessTokenObject, AuthPayload,
    BranchProtectionRuleObject, DevWorkspaceObject, IssueCommentObject, IssueObject, LabelObject,
    MilestoneObject, NotificationObject, OrganizationObject,
    PrReviewCommentObject, PrReviewObject, ProjectCardObject, ProjectColumnObject, ProjectObject,
    PullRequestObject, RepositoryObject, UserObject, WebhookObject,
    WorkflowRunObject,
};

pub struct MutationRoot;

/// Best-effort notification insert: logs and swallows any error so that a
/// failure to notify never fails the mutation that triggered it.
async fn notify(
    app: &AppContext,
    user_id: Uuid,
    kind: &str,
    repo_id: Uuid,
    subject_id: Uuid,
    message: String,
) {
    let id = Uuid::new_v4();
    let created_at = Utc::now();
    let res = app
        .db
        .execute(
            "INSERT INTO notifications (id, user_id, kind, repo_id, subject_id, message, read_at, created_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL, ?7)",
            params!(
                id.to_string(),
                user_id.to_string(),
                kind.to_string(),
                repo_id.to_string(),
                subject_id.to_string(),
                message,
                created_at.to_rfc3339()
            ),
        )
        .await;
    if let Err(e) = res {
        tracing::warn!("failed to insert notification: {e}");
    }
}

/// Best-effort activity feed insert: logs and swallows any error so that a
/// failure to record activity never fails the mutation that triggered it.
async fn record_activity(app: &AppContext, repo_id: Option<Uuid>, actor_id: Uuid, kind: &str, summary: String) {
    let id = Uuid::new_v4();
    let created_at = Utc::now();
    let res = app
        .db
        .execute(
            "INSERT INTO activity_events (id, repo_id, actor_id, kind, summary, created_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params!(
                id.to_string(),
                repo_id.map(|r| r.to_string()),
                actor_id.to_string(),
                kind.to_string(),
                summary,
                created_at.to_rfc3339()
            ),
        )
        .await;
    if let Err(e) = res {
        tracing::warn!("failed to insert activity event: {e}");
    }
}

/// Enqueues a `dev_workspace_action` job (`kind::DEV_WORKSPACE_ACTION`) for
/// a standalone runner to claim via `POST /runner/claim`, merging `action`
/// into `payload` under the `"action"` key. Returns the new job's id.
async fn enqueue_dev_workspace_job(
    app: &AppContext,
    action: &str,
    payload: &serde_json::Value,
) -> async_graphql::Result<Uuid> {
    let job_id = Uuid::new_v4();
    let mut full_payload = payload.clone();
    if let Some(obj) = full_payload.as_object_mut() {
        obj.insert("action".to_string(), serde_json::Value::String(action.to_string()));
    }
    app.db
        .execute(
            "INSERT INTO runner_jobs (id, kind, repo_id, workflow_run_id, payload, status, claimed_by, claimed_at, created_at, finished_at, result) \
             VALUES (?1, ?2, NULL, NULL, ?3, ?4, NULL, NULL, ?5, NULL, NULL)",
            params!(
                job_id.to_string(),
                entity::runner_job::kind::DEV_WORKSPACE_ACTION.to_string(),
                full_payload.to_string(),
                entity::runner_job::status::QUEUED.to_string(),
                Utc::now().to_rfc3339()
            ),
        )
        .await?;
    Ok(job_id)
}

/// Polls `runner_jobs` for a job enqueued by `enqueue_dev_workspace_job` to
/// reach a terminal status, up to `timeout`. This is plain DB polling
/// rather than a push notification -- there's no pub/sub infrastructure in
/// this codebase -- so it trades a little latency (bounded by the poll
/// interval below, well under the runner's own ~5s claim-loop interval)
/// for simplicity.
async fn wait_for_runner_job(
    app: &AppContext,
    job_id: Uuid,
    timeout: std::time::Duration,
) -> async_graphql::Result<entity::runner_job::Model> {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        let job = app
            .db
            .query_as::<entity::runner_job::Model, _>(
                "SELECT * FROM runner_jobs WHERE id = ?1",
                params!(job_id.to_string()),
            )
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| async_graphql::Error::new("runner job disappeared"))?;
        if job.status == entity::runner_job::status::SUCCESS || job.status == entity::runner_job::status::FAILURE {
            return Ok(job);
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(async_graphql::Error::new(
                "timed out waiting for a standalone runner to claim this dev workspace action -- is one connected?",
            ));
        }
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    }
}

/// Extracts `(status, container_id, runner_id)` from a completed `create`
/// dev-workspace job. On failure (or an unparseable/missing result), status
/// is `entity::dev_workspace::status::ERROR` with both ids left `None`.
fn apply_create_result(job: &entity::runner_job::Model) -> (String, Option<String>, Option<String>) {
    if job.status != entity::runner_job::status::SUCCESS {
        return (entity::dev_workspace::status::ERROR.to_string(), None, None);
    }
    let Some(result) = &job.result else {
        return (entity::dev_workspace::status::ERROR.to_string(), None, None);
    };
    let Ok(parsed) = serde_json::from_str::<serde_json::Value>(result) else {
        return (entity::dev_workspace::status::ERROR.to_string(), None, None);
    };
    let container_id = parsed.get("container_id").and_then(|v| v.as_str()).map(str::to_string);
    let runner_id = parsed.get("runner_id").and_then(|v| v.as_str()).map(str::to_string);
    if container_id.is_none() {
        return (entity::dev_workspace::status::ERROR.to_string(), None, None);
    }
    (entity::dev_workspace::status::RUNNING.to_string(), container_id, runner_id)
}

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
        app.db
            .query_as::<entity::org_member::Model, _>(
                "SELECT * FROM org_members WHERE org_id = ?1 AND user_id = ?2",
                params!(repo.owner_id.to_string(), user_id.to_string()),
            )
            .await?
            .into_iter()
            .next()
            .map(|m| m.role == entity::org_member::role::OWNER || m.role == entity::org_member::role::ADMIN)
            .unwrap_or(false)
    } else {
        false
    };

    let collaborator_perm = app
        .db
        .query_as::<entity::repo_collaborator::Model, _>(
            "SELECT * FROM repo_collaborators WHERE repo_id = ?1 AND user_id = ?2",
            params!(repo.id.to_string(), user_id.to_string()),
        )
        .await?
        .into_iter()
        .next()
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

/// Errors unless the given user is an owner or admin of the organization
/// (used to gate org membership management mutations).
async fn require_org_admin(app: &AppContext, org_id: Uuid, user_id: Uuid) -> async_graphql::Result<()> {
    let is_admin = app
        .db
        .query_as::<entity::org_member::Model, _>(
            "SELECT * FROM org_members WHERE org_id = ?1 AND user_id = ?2",
            params!(org_id.to_string(), user_id.to_string()),
        )
        .await?
        .into_iter()
        .next()
        .map(|m| m.role == entity::org_member::role::OWNER || m.role == entity::org_member::role::ADMIN)
        .unwrap_or(false);
    if !is_admin {
        return Err(async_graphql::Error::new("forbidden"));
    }
    Ok(())
}

async fn find_repo(app: &AppContext, repo_id: Uuid) -> async_graphql::Result<entity::repository::Model> {
    app.db
        .query_as::<entity::repository::Model, _>(
            "SELECT * FROM repositories WHERE id = ?1",
            params!(repo_id.to_string()),
        )
        .await?
        .into_iter()
        .next()
        .ok_or_else(|| async_graphql::Error::new("repository not found"))
}

/// Loads and decrypts the `SECRETS_ENCRYPTION_KEY` environment variable into
/// a raw 32-byte key. Returns an error if unset or malformed.
fn secrets_encryption_key() -> anyhow::Result<[u8; 32]> {
    let encoded = std::env::var("SECRETS_ENCRYPTION_KEY")
        .map_err(|_| anyhow::anyhow!("SECRETS_ENCRYPTION_KEY environment variable must be set"))?;
    actions::key_from_base64(&encoded)
}

/// Loads all secrets for a repository, decrypting each value, for injection
/// into a workflow job run. Best-effort: individual secrets that fail to
/// decrypt are skipped with a warning rather than aborting the whole run.
pub(crate) async fn load_repo_secrets(
    app: &AppContext,
    repo_id: Uuid,
) -> anyhow::Result<std::collections::HashMap<String, String>> {
    let key = secrets_encryption_key()?;
    let rows = app
        .db
        .query_as::<entity::repo_secret::Model, _>(
            "SELECT * FROM repo_secrets WHERE repo_id = ?1",
            params!(repo_id.to_string()),
        )
        .await?;

    let mut out = std::collections::HashMap::new();
    for row in rows {
        match actions::decrypt_secret(&key, &row.encrypted_value) {
            Ok(value) => {
                out.insert(row.name, value);
            }
            Err(e) => {
                tracing::warn!("failed to decrypt secret {} for repo {repo_id}: {e}", row.name);
            }
        }
    }
    Ok(out)
}

#[Object]
impl MutationRoot {
    /// Creates a new user account with an argon2-hashed password.
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

        let id = Uuid::new_v4();
        let created_at = Utc::now();
        app.db
            .execute(
                "INSERT INTO users (id, username, email, password_hash, is_admin, avatar_url, created_at, deactivated_at) \
                 VALUES (?1, ?2, ?3, ?4, 0, NULL, ?5, NULL)",
                params!(id.to_string(), username.clone(), email.clone(), password_hash.clone(), created_at.to_rfc3339()),
            )
            .await?;

        let user = entity::user::Model {
            id,
            username,
            email,
            password_hash,
            is_admin: false,
            avatar_url: None,
            created_at,
            deactivated_at: None,
        };
        Ok(UserObject::from(user))
    }

    /// Register an SSH public key for the current user, enabling
    /// `git clone`/`push` over SSH against the ssh-server (default port
    /// 2222) using that key for authentication.
    async fn add_ssh_key(
        &self,
        ctx: &Context<'_>,
        title: String,
        public_key: String,
    ) -> async_graphql::Result<crate::types::SshKeyObject> {
        let req = ctx.data::<RequestContext>()?;
        let claims = require_user(req)?;
        let app = ctx.data::<AppContext>()?;

        let fingerprint = auth::ssh_key_fingerprint(public_key.trim())
            .map_err(|e| async_graphql::Error::new(format!("invalid SSH public key: {e}")))?;

        let id = Uuid::new_v4();
        let created_at = Utc::now();
        app.db
            .execute(
                "INSERT INTO ssh_keys (id, user_id, title, public_key, fingerprint, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params!(
                    id.to_string(),
                    claims.sub.to_string(),
                    title.clone(),
                    public_key.clone(),
                    fingerprint.clone(),
                    created_at.to_rfc3339()
                ),
            )
            .await?;

        let key = entity::ssh_key::Model {
            id,
            user_id: claims.sub,
            title,
            public_key,
            fingerprint,
            created_at,
        };
        Ok(crate::types::SshKeyObject::from(key))
    }

    /// Remove one of the current user's SSH keys.
    async fn remove_ssh_key(&self, ctx: &Context<'_>, key_id: Uuid) -> async_graphql::Result<bool> {
        let req = ctx.data::<RequestContext>()?;
        let claims = require_user(req)?;
        let app = ctx.data::<AppContext>()?;

        let affected = app
            .db
            .execute(
                "DELETE FROM ssh_keys WHERE id = ?1 AND user_id = ?2",
                params!(key_id.to_string(), claims.sub.to_string()),
            )
            .await?;
        Ok(affected > 0)
    }

    /// Verifies username/password and returns a signed JWT plus the user.
    async fn login(
        &self,
        ctx: &Context<'_>,
        username: String,
        password: String,
    ) -> async_graphql::Result<AuthPayload> {
        let app = ctx.data::<AppContext>()?;
        let user = app
            .db
            .query_as::<entity::user::Model, _>(
                "SELECT * FROM users WHERE username = ?1",
                params!(username),
            )
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| async_graphql::Error::new("invalid username or password"))?;

        if user.deactivated_at.is_some() {
            return Err(async_graphql::Error::new("account deactivated"));
        }

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

    /// Grants or revokes site-admin status for a user. Restricted to
    /// existing admins.
    async fn admin_set_user_admin(
        &self,
        ctx: &Context<'_>,
        user_id: Uuid,
        is_admin: bool,
    ) -> async_graphql::Result<UserObject> {
        let app = ctx.data::<AppContext>()?;
        let req = ctx.data::<RequestContext>()?;
        let claims = require_user(req)?;
        if !claims.is_admin {
            return Err(async_graphql::Error::new("forbidden"));
        }

        let mut user = app
            .db
            .query_as::<entity::user::Model, _>("SELECT * FROM users WHERE id = ?1", params!(user_id.to_string()))
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| async_graphql::Error::new("user not found"))?;

        app.db
            .execute(
                "UPDATE users SET is_admin = ?1 WHERE id = ?2",
                params!(is_admin, user_id.to_string()),
            )
            .await?;
        user.is_admin = is_admin;

        Ok(UserObject::from(user))
    }

    /// Deactivates a user account (blocks future logins). Restricted to
    /// admins.
    async fn admin_deactivate_user(&self, ctx: &Context<'_>, user_id: Uuid) -> async_graphql::Result<UserObject> {
        let app = ctx.data::<AppContext>()?;
        let req = ctx.data::<RequestContext>()?;
        let claims = require_user(req)?;
        if !claims.is_admin {
            return Err(async_graphql::Error::new("forbidden"));
        }

        let mut user = app
            .db
            .query_as::<entity::user::Model, _>("SELECT * FROM users WHERE id = ?1", params!(user_id.to_string()))
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| async_graphql::Error::new("user not found"))?;

        let deactivated_at = Utc::now();
        app.db
            .execute(
                "UPDATE users SET deactivated_at = ?1 WHERE id = ?2",
                params!(deactivated_at.to_rfc3339(), user_id.to_string()),
            )
            .await?;
        user.deactivated_at = Some(deactivated_at);

        Ok(UserObject::from(user))
    }

    /// Writes (or re-writes) the branch-protection pre-receive hook onto
    /// every repository in the instance. `init_repo` already writes this
    /// hook for newly-created repos; this is the one-off maintenance
    /// operation for backfilling it onto repos that predate
    /// branch-protection support. Safe to run more than once -- it's a
    /// plain overwrite for every repo, not just ones missing the hook.
    /// Returns the number of repos it wrote the hook for; a repo whose
    /// on-disk directory can't be found (or written to) is logged and
    /// skipped rather than failing the whole operation.
    async fn admin_backfill_pre_receive_hooks(&self, ctx: &Context<'_>) -> async_graphql::Result<i32> {
        let app = ctx.data::<AppContext>()?;
        let req = ctx.data::<RequestContext>()?;
        let claims = require_user(req)?;
        if !claims.is_admin {
            return Err(async_graphql::Error::new("forbidden"));
        }

        let repos = app
            .db
            .query_as::<entity::repository::Model, _>("SELECT * FROM repositories", params!())
            .await?;

        let mut count = 0i32;
        for repo in repos {
            let owner_login = resolve_owner_login(&app.db, &repo.owner_type, repo.owner_id)
                .await
                .unwrap_or_default();
            match app.repo_manager.ensure_pre_receive_hook(&owner_login, &repo.name) {
                Ok(()) => count += 1,
                Err(e) => tracing::warn!(
                    "failed to backfill pre-receive hook for {owner_login}/{}: {e}",
                    repo.name
                ),
            }
        }

        Ok(count)
    }

    /// Creates a repository owned by the current user, plus its bare git repo on disk.
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

        let owner = app
            .db
            .query_as::<entity::user::Model, _>("SELECT * FROM users WHERE id = ?1", params!(claims.sub.to_string()))
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| async_graphql::Error::new("owner not found"))?;

        app.repo_manager
            .init_repo(&owner.username, &name)
            .map_err(|e| async_graphql::Error::new(e.to_string()))?;

        if let Err(e) = app.repo_manager.init_wiki(&owner.username, &name) {
            tracing::warn!("failed to initialize wiki repo for {}/{name}: {e}", owner.username);
        }

        let id = Uuid::new_v4();
        let created_at = Utc::now();
        let default_branch = "main".to_string();
        app.db
            .execute(
                "INSERT INTO repositories (id, owner_type, owner_id, name, description, is_private, default_branch, created_at, forked_from_id) \
                 VALUES (?1, 'user', ?2, ?3, ?4, ?5, ?6, ?7, NULL)",
                params!(
                    id.to_string(),
                    owner.id.to_string(),
                    name.clone(),
                    description.clone(),
                    is_private,
                    default_branch.clone(),
                    created_at.to_rfc3339()
                ),
            )
            .await?;

        let repo = entity::repository::Model {
            id,
            owner_type: "user".to_string(),
            owner_id: owner.id,
            name,
            description,
            is_private,
            default_branch,
            created_at,
            forked_from_id: None,
        };

        record_activity(
            app,
            Some(repo.id),
            claims.sub,
            entity::activity_event::kind::REPO_CREATED,
            format!("{} created repository {}", owner.username, repo.name),
        )
        .await;

        Ok(RepositoryObject::from_model(&app.db, repo).await)
    }

    /// Deletes a repository and its on-disk bare repo/wiki. Requires Admin permission.
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
        let _ = app.repo_manager.delete_wiki(&owner_login, &repo.name);

        app.db
            .execute("DELETE FROM repositories WHERE id = ?1", params!(repo.id.to_string()))
            .await?;

        Ok(true)
    }

    /// Renames a repository: updates the DB `name` column and moves both
    /// the bare repo directory and (if present) its wiki directory on disk
    /// to match. Requires Admin permission on the repository. Errors if the
    /// owner already has another repository named `new_name`. On partial
    /// failure (e.g. the wiki move or the DB update fails after the repo
    /// directory was already moved), best-effort rolls back the disk
    /// rename(s) already performed so disk and DB don't end up disagreeing
    /// about the repo's name.
    async fn rename_repository(
        &self,
        ctx: &Context<'_>,
        repo_id: Uuid,
        new_name: String,
    ) -> async_graphql::Result<RepositoryObject> {
        let app = ctx.data::<AppContext>()?;
        let req = ctx.data::<RequestContext>()?;
        let claims = require_user(req)?;

        let mut repo = find_repo(app, repo_id).await?;
        let perm = repo_permission(app, &repo, claims.sub).await?;
        if perm != Some(Permission::Admin) {
            return Err(async_graphql::Error::new("forbidden"));
        }

        if new_name == repo.name {
            return Ok(RepositoryObject::from_model(&app.db, repo).await);
        }

        let existing = app
            .db
            .query_as::<entity::repository::Model, _>(
                "SELECT * FROM repositories WHERE owner_type = ?1 AND owner_id = ?2 AND name = ?3",
                params!(repo.owner_type.clone(), repo.owner_id.to_string(), new_name.clone()),
            )
            .await?
            .into_iter()
            .next();
        if existing.is_some() {
            return Err(async_graphql::Error::new(format!(
                "owner already has a repository named '{new_name}'"
            )));
        }

        let owner_login = resolve_owner_login(&app.db, &repo.owner_type, repo.owner_id)
            .await
            .unwrap_or_default();
        let old_name = repo.name.clone();

        app.repo_manager
            .rename_repo(&owner_login, &old_name, &new_name)
            .map_err(|e| async_graphql::Error::new(e.to_string()))?;

        if let Err(e) = app.repo_manager.rename_wiki(&owner_login, &old_name, &new_name) {
            let _ = app.repo_manager.rename_repo(&owner_login, &new_name, &old_name);
            return Err(async_graphql::Error::new(e.to_string()));
        }

        if let Err(e) = app
            .db
            .execute(
                "UPDATE repositories SET name = ?1 WHERE id = ?2",
                params!(new_name.clone(), repo_id.to_string()),
            )
            .await
        {
            let _ = app.repo_manager.rename_wiki(&owner_login, &new_name, &old_name);
            let _ = app.repo_manager.rename_repo(&owner_login, &new_name, &old_name);
            return Err(e.into());
        }

        repo.name = new_name;
        Ok(RepositoryObject::from_model(&app.db, repo).await)
    }

    /// Updates a repository's mutable metadata (description, visibility,
    /// default branch). Requires Admin permission on the repository. Use
    /// `renameRepository` to change its name.
    async fn update_repository(
        &self,
        ctx: &Context<'_>,
        repo_id: Uuid,
        description: Option<String>,
        is_private: Option<bool>,
        default_branch: Option<String>,
    ) -> async_graphql::Result<RepositoryObject> {
        let app = ctx.data::<AppContext>()?;
        let req = ctx.data::<RequestContext>()?;
        let claims = require_user(req)?;

        let mut repo = find_repo(app, repo_id).await?;
        let perm = repo_permission(app, &repo, claims.sub).await?;
        if perm != Some(Permission::Admin) {
            return Err(async_graphql::Error::new("forbidden"));
        }

        if let Some(description) = description {
            app.db
                .execute(
                    "UPDATE repositories SET description = ?1 WHERE id = ?2",
                    params!(description.clone(), repo_id.to_string()),
                )
                .await?;
            repo.description = Some(description);
        }
        if let Some(is_private) = is_private {
            app.db
                .execute(
                    "UPDATE repositories SET is_private = ?1 WHERE id = ?2",
                    params!(is_private, repo_id.to_string()),
                )
                .await?;
            repo.is_private = is_private;
        }
        if let Some(default_branch) = default_branch {
            app.db
                .execute(
                    "UPDATE repositories SET default_branch = ?1 WHERE id = ?2",
                    params!(default_branch.clone(), repo_id.to_string()),
                )
                .await?;
            repo.default_branch = default_branch;
        }

        Ok(RepositoryObject::from_model(&app.db, repo).await)
    }

    /// Forks a repository into a new repository owned by the current user,
    /// with the same name as the source. Errors if the current user already
    /// owns a repository with that name.
    async fn fork_repository(&self, ctx: &Context<'_>, repo_id: Uuid) -> async_graphql::Result<RepositoryObject> {
        let app = ctx.data::<AppContext>()?;
        let req = ctx.data::<RequestContext>()?;
        let claims = require_user(req)?;

        let source_repo = find_repo(app, repo_id).await?;
        let perm = repo_permission(app, &source_repo, claims.sub).await?;
        if perm.is_none() {
            return Err(async_graphql::Error::new("forbidden"));
        }

        let dest_owner = app
            .db
            .query_as::<entity::user::Model, _>("SELECT * FROM users WHERE id = ?1", params!(claims.sub.to_string()))
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| async_graphql::Error::new("user not found"))?;

        let existing = app
            .db
            .query_as::<entity::repository::Model, _>(
                "SELECT * FROM repositories WHERE owner_type = 'user' AND owner_id = ?1 AND name = ?2",
                params!(dest_owner.id.to_string(), source_repo.name.clone()),
            )
            .await?
            .into_iter()
            .next();
        if existing.is_some() {
            return Err(async_graphql::Error::new(format!(
                "you already own a repository named '{}'",
                source_repo.name
            )));
        }

        let source_owner_login =
            resolve_owner_login(&app.db, &source_repo.owner_type, source_repo.owner_id)
                .await
                .unwrap_or_default();

        app.repo_manager
            .fork_repo(&source_owner_login, &source_repo.name, &dest_owner.username, &source_repo.name)
            .map_err(|e| async_graphql::Error::new(e.to_string()))?;

        let id = Uuid::new_v4();
        let created_at = Utc::now();
        app.db
            .execute(
                "INSERT INTO repositories (id, owner_type, owner_id, name, description, is_private, default_branch, created_at, forked_from_id) \
                 VALUES (?1, 'user', ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params!(
                    id.to_string(),
                    dest_owner.id.to_string(),
                    source_repo.name.clone(),
                    source_repo.description.clone(),
                    source_repo.is_private,
                    source_repo.default_branch.clone(),
                    created_at.to_rfc3339(),
                    source_repo.id.to_string()
                ),
            )
            .await?;

        let repo = entity::repository::Model {
            id,
            owner_type: "user".to_string(),
            owner_id: dest_owner.id,
            name: source_repo.name.clone(),
            description: source_repo.description.clone(),
            is_private: source_repo.is_private,
            default_branch: source_repo.default_branch.clone(),
            created_at,
            forked_from_id: Some(source_repo.id),
        };

        record_activity(
            app,
            Some(repo.id),
            claims.sub,
            entity::activity_event::kind::REPO_CREATED,
            format!("{} forked repository {}", dest_owner.username, repo.name),
        )
        .await;

        Ok(RepositoryObject::from_model(&app.db, repo).await)
    }

    /// Opens a new issue on a repository. Requires at least Read permission.
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

        let existing_count = app
            .db
            .query_as::<entity::issue::Model, _>("SELECT * FROM issues WHERE repo_id = ?1", params!(repo_id.to_string()))
            .await?
            .len();

        let id = Uuid::new_v4();
        let number = existing_count as i32 + 1;
        let created_at = Utc::now();
        let state = entity::issue::state::OPEN.to_string();
        app.db
            .execute(
                "INSERT INTO issues (id, repo_id, number, title, body, author_id, state, created_at, closed_at, milestone_id) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, NULL, NULL)",
                params!(
                    id.to_string(),
                    repo_id.to_string(),
                    number,
                    title.clone(),
                    body.clone(),
                    claims.sub.to_string(),
                    state.clone(),
                    created_at.to_rfc3339()
                ),
            )
            .await?;

        let issue = entity::issue::Model {
            id,
            repo_id,
            number,
            title,
            body,
            author_id: claims.sub,
            state,
            created_at,
            closed_at: None,
            milestone_id: None,
        };

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

        record_activity(
            app,
            Some(repo.id),
            claims.sub,
            entity::activity_event::kind::ISSUE_OPENED,
            format!("{} opened issue #{} on {}", claims.username, issue.number, repo.name),
        )
        .await;

        Ok(IssueObject::from(issue))
    }

    /// Updates an issue's title/body, and/or opens or closes it via `state`
    /// (one of `"open"`/`"closed"`). Requires at least Write permission on
    /// the parent repository.
    async fn update_issue(
        &self,
        ctx: &Context<'_>,
        issue_id: Uuid,
        title: Option<String>,
        body: Option<String>,
        state: Option<String>,
    ) -> async_graphql::Result<IssueObject> {
        let app = ctx.data::<AppContext>()?;
        let req = ctx.data::<RequestContext>()?;
        let claims = require_user(req)?;

        let mut issue = app
            .db
            .query_as::<entity::issue::Model, _>("SELECT * FROM issues WHERE id = ?1", params!(issue_id.to_string()))
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| async_graphql::Error::new("issue not found"))?;
        let repo = find_repo(app, issue.repo_id).await?;
        let perm = repo_permission(app, &repo, claims.sub).await?;
        if perm < Some(Permission::Write) {
            return Err(async_graphql::Error::new("forbidden"));
        }

        if let Some(state) = &state {
            let valid_states = [entity::issue::state::OPEN, entity::issue::state::CLOSED];
            if !valid_states.contains(&state.as_str()) {
                return Err(async_graphql::Error::new(format!(
                    "invalid issue state '{state}'; expected one of {valid_states:?}"
                )));
            }
        }

        let was_open = issue.state == entity::issue::state::OPEN;
        if let Some(title) = title {
            app.db
                .execute(
                    "UPDATE issues SET title = ?1 WHERE id = ?2",
                    params!(title.clone(), issue_id.to_string()),
                )
                .await?;
            issue.title = title;
        }
        if let Some(body) = body {
            app.db
                .execute(
                    "UPDATE issues SET body = ?1 WHERE id = ?2",
                    params!(body.clone(), issue_id.to_string()),
                )
                .await?;
            issue.body = Some(body);
        }
        if let Some(state) = state {
            let now_closed = state == entity::issue::state::CLOSED;
            let closed_at = if now_closed { Some(Utc::now()) } else { None };
            app.db
                .execute(
                    "UPDATE issues SET state = ?1, closed_at = ?2 WHERE id = ?3",
                    params!(state.clone(), closed_at.map(|c: chrono::DateTime<Utc>| c.to_rfc3339()), issue_id.to_string()),
                )
                .await?;
            issue.state = state;
            issue.closed_at = closed_at;
        }

        let action = if issue.state == entity::issue::state::CLOSED && was_open {
            "closed"
        } else if issue.state == entity::issue::state::OPEN && !was_open {
            "reopened"
        } else {
            "edited"
        };
        let payload = serde_json::json!({
            "action": action,
            "issue": {
                "id": issue.id,
                "number": issue.number,
                "title": issue.title,
                "state": issue.state,
            },
            "repository": { "id": repo.id, "name": repo.name },
        });
        let _ = app.webhook_dispatcher.dispatch(repo.id, "issues", payload).await;

        Ok(IssueObject::from(issue))
    }

    /// Posts a comment on an issue.
    async fn comment_on_issue(
        &self,
        ctx: &Context<'_>,
        issue_id: Uuid,
        body: String,
    ) -> async_graphql::Result<IssueCommentObject> {
        let app = ctx.data::<AppContext>()?;
        let req = ctx.data::<RequestContext>()?;
        let claims = require_user(req)?;

        let issue = app
            .db
            .query_as::<entity::issue::Model, _>("SELECT * FROM issues WHERE id = ?1", params!(issue_id.to_string()))
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| async_graphql::Error::new("issue not found"))?;
        let repo = find_repo(app, issue.repo_id).await?;
        let perm = repo_permission(app, &repo, claims.sub).await?;
        if perm.is_none() {
            return Err(async_graphql::Error::new("forbidden"));
        }

        let id = Uuid::new_v4();
        let created_at = Utc::now();
        app.db
            .execute(
                "INSERT INTO issue_comments (id, issue_id, author_id, body, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
                params!(id.to_string(), issue_id.to_string(), claims.sub.to_string(), body.clone(), created_at.to_rfc3339()),
            )
            .await?;

        let comment = entity::issue_comment::Model {
            id,
            issue_id,
            author_id: claims.sub,
            body,
            created_at,
        };

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

        if issue.author_id != claims.sub {
            notify(
                app,
                issue.author_id,
                entity::notification::kind::ISSUE_COMMENT,
                repo.id,
                issue.id,
                format!("{} commented on issue #{}: {}", claims.username, issue.number, issue.title),
            )
            .await;
        }

        Ok(IssueCommentObject::from(comment))
    }

    /// Opens a pull request proposing to merge `source_branch` into `target_branch`.
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

        let existing_count = app
            .db
            .query_as::<entity::pull_request::Model, _>(
                "SELECT * FROM pull_requests WHERE repo_id = ?1",
                params!(repo_id.to_string()),
            )
            .await?
            .len();

        let id = Uuid::new_v4();
        let number = existing_count as i32 + 1;
        let created_at = Utc::now();
        let state = entity::pull_request::state::OPEN.to_string();
        app.db
            .execute(
                "INSERT INTO pull_requests (id, repo_id, number, title, body, author_id, source_branch, target_branch, state, created_at, merged_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, NULL)",
                params!(
                    id.to_string(),
                    repo_id.to_string(),
                    number,
                    title.clone(),
                    body.clone(),
                    claims.sub.to_string(),
                    source_branch.clone(),
                    target_branch.clone(),
                    state.clone(),
                    created_at.to_rfc3339()
                ),
            )
            .await?;

        let pr = entity::pull_request::Model {
            id,
            repo_id,
            number,
            title,
            body,
            author_id: claims.sub,
            source_branch,
            target_branch,
            state,
            created_at,
            merged_at: None,
        };

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

        record_activity(
            app,
            Some(repo.id),
            claims.sub,
            entity::activity_event::kind::PR_OPENED,
            format!("{} opened pull request #{} on {}", claims.username, pr.number, repo.name),
        )
        .await;

        Ok(PullRequestObject::from(pr))
    }

    /// Merges a pull request: performs an actual git merge (fast-forward or
    /// merge commit) of `source_branch` into `target_branch` via git-core,
    /// then marks the PR as merged in the database.
    async fn merge_pull_request(
        &self,
        ctx: &Context<'_>,
        pr_id: Uuid,
        merge_method: Option<String>,
    ) -> async_graphql::Result<PullRequestObject> {
        let app = ctx.data::<AppContext>()?;
        let req = ctx.data::<RequestContext>()?;
        let claims = require_user(req)?;

        let mut pr = app
            .db
            .query_as::<entity::pull_request::Model, _>("SELECT * FROM pull_requests WHERE id = ?1", params!(pr_id.to_string()))
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| async_graphql::Error::new("pull request not found"))?;
        let repo = find_repo(app, pr.repo_id).await?;
        let perm = repo_permission(app, &repo, claims.sub).await?;
        if perm < Some(Permission::Write) {
            return Err(async_graphql::Error::new("forbidden"));
        }

        // Enforce branch protection: if any active rule for this repo matches
        // the PR's target branch and requires a minimum number of approved
        // reviews, count the approved `pr_reviews` rows for this PR and
        // refuse to merge if the threshold isn't met.
        let rules = app
            .db
            .query_as::<entity::branch_protection_rule::Model, _>(
                "SELECT * FROM branch_protection_rules WHERE repo_id = ?1",
                params!(repo.id.to_string()),
            )
            .await?;
        let matching_rule = rules.into_iter().find(|r| {
            entity::branch_protection_rule::branch_matches_pattern(&r.branch_pattern, &pr.target_branch)
        });
        if let Some(rule) = matching_rule {
            if rule.require_reviews_count > 0 {
                let approved_count = app
                    .db
                    .query_as::<entity::pr_review::Model, _>(
                        "SELECT * FROM pr_reviews WHERE pr_id = ?1 AND state = ?2",
                        params!(pr.id.to_string(), entity::pr_review::state::APPROVED),
                    )
                    .await?
                    .len() as i32;
                if approved_count < rule.require_reviews_count {
                    return Err(async_graphql::Error::new(format!(
                        "branch protection: '{}' requires {} approved review(s), but only {} found",
                        rule.branch_pattern, rule.require_reviews_count, approved_count
                    )));
                }
            }
        }

        let owner_login = resolve_owner_login(&app.db, &repo.owner_type, repo.owner_id)
            .await
            .unwrap_or_default();

        let message = format!(
            "Merge pull request #{} from {}",
            pr.number, pr.source_branch
        );

        let author_email = format!("{}@users.noreply.local", claims.username);

        let merge_method = merge_method.unwrap_or_else(|| "merge".to_string());
        let merge_result = match merge_method.as_str() {
            "merge" => app.repo_manager.merge_branches(
                &owner_login,
                &repo.name,
                &pr.source_branch,
                &pr.target_branch,
                &claims.username,
                &author_email,
                &message,
            ),
            "squash" => app.repo_manager.squash_merge(
                &owner_login,
                &repo.name,
                &pr.source_branch,
                &pr.target_branch,
                &claims.username,
                &author_email,
                &message,
            ),
            "rebase" => app.repo_manager.rebase_merge(
                &owner_login,
                &repo.name,
                &pr.source_branch,
                &pr.target_branch,
                &claims.username,
                &author_email,
            ),
            other => {
                return Err(async_graphql::Error::new(format!(
                    "unknown merge method '{other}'; expected 'merge', 'squash', or 'rebase'"
                )));
            }
        };

        match merge_result {
            Ok(_) => {}
            Err(git_core::GitCoreError::MergeConflict(_, _)) => {
                return Err(async_graphql::Error::new(
                    "merge conflict, cannot merge automatically",
                ));
            }
            Err(e) => return Err(async_graphql::Error::new(e.to_string())),
        }

        let state = entity::pull_request::state::MERGED.to_string();
        let merged_at = Utc::now();
        app.db
            .execute(
                "UPDATE pull_requests SET state = ?1, merged_at = ?2 WHERE id = ?3",
                params!(state.clone(), merged_at.to_rfc3339(), pr.id.to_string()),
            )
            .await?;
        pr.state = state;
        pr.merged_at = Some(merged_at);

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

        if pr.author_id != claims.sub {
            notify(
                app,
                pr.author_id,
                entity::notification::kind::PR_MERGED,
                repo.id,
                pr.id,
                format!("{} merged your pull request #{}: {}", claims.username, pr.number, pr.title),
            )
            .await;
        }
        record_activity(
            app,
            Some(repo.id),
            claims.sub,
            entity::activity_event::kind::PR_MERGED,
            format!("{} merged pull request #{} on {}", claims.username, pr.number, repo.name),
        )
        .await;

        Ok(PullRequestObject::from(pr))
    }

    /// Closes a pull request without merging it. Requires at least Write
    /// permission on the repository. No-op error if the PR is already
    /// merged or closed.
    async fn close_pull_request(&self, ctx: &Context<'_>, pr_id: Uuid) -> async_graphql::Result<PullRequestObject> {
        let app = ctx.data::<AppContext>()?;
        let req = ctx.data::<RequestContext>()?;
        let claims = require_user(req)?;

        let mut pr = app
            .db
            .query_as::<entity::pull_request::Model, _>("SELECT * FROM pull_requests WHERE id = ?1", params!(pr_id.to_string()))
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| async_graphql::Error::new("pull request not found"))?;
        let repo = find_repo(app, pr.repo_id).await?;
        let perm = repo_permission(app, &repo, claims.sub).await?;
        if perm < Some(Permission::Write) {
            return Err(async_graphql::Error::new("forbidden"));
        }
        if pr.state != entity::pull_request::state::OPEN {
            return Err(async_graphql::Error::new(format!(
                "cannot close pull request in state '{}'",
                pr.state
            )));
        }

        let state = entity::pull_request::state::CLOSED.to_string();
        app.db
            .execute(
                "UPDATE pull_requests SET state = ?1 WHERE id = ?2",
                params!(state.clone(), pr.id.to_string()),
            )
            .await?;
        pr.state = state;

        let payload = serde_json::json!({
            "action": "closed",
            "pull_request": {
                "id": pr.id,
                "number": pr.number,
                "title": pr.title,
                "state": pr.state,
                "merged": false,
            },
            "repository": { "id": repo.id, "name": repo.name },
        });
        let _ = app
            .webhook_dispatcher
            .dispatch(repo.id, "pull_request", payload)
            .await;

        record_activity(
            app,
            Some(repo.id),
            claims.sub,
            entity::activity_event::kind::PR_CLOSED,
            format!("{} closed pull request #{} on {}", claims.username, pr.number, repo.name),
        )
        .await;

        Ok(PullRequestObject::from(pr))
    }

    /// Creates or updates a wiki page (`{page}.md`) for a repository,
    /// committing the change on the wiki repo's default branch. Requires
    /// at least Write permission on the parent repository.
    async fn write_wiki_page(
        &self,
        ctx: &Context<'_>,
        repo_id: Uuid,
        page: String,
        content: String,
        message: String,
    ) -> async_graphql::Result<String> {
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

        let author_email = format!("{}@users.noreply.local", claims.username);

        let sha = app
            .repo_manager
            .wiki_write_page(
                &owner_login,
                &repo.name,
                &page,
                &content,
                &claims.username,
                &author_email,
                &message,
            )
            .map_err(|e| async_graphql::Error::new(e.to_string()))?;

        Ok(sha)
    }

    /// Creates a branch protection rule for a repository. `branch_pattern`
    /// supports a literal branch name (e.g. "main") or a trailing wildcard
    /// (e.g. "release/*"). Requires Admin permission on the repository.
    async fn create_branch_protection_rule(
        &self,
        ctx: &Context<'_>,
        repo_id: Uuid,
        branch_pattern: String,
        #[graphql(default)] require_reviews_count: i32,
        #[graphql(default = true)] block_force_push: bool,
    ) -> async_graphql::Result<BranchProtectionRuleObject> {
        let app = ctx.data::<AppContext>()?;
        let req = ctx.data::<RequestContext>()?;
        let claims = require_user(req)?;

        let repo = find_repo(app, repo_id).await?;
        let perm = repo_permission(app, &repo, claims.sub).await?;
        if perm != Some(Permission::Admin) {
            return Err(async_graphql::Error::new("forbidden"));
        }

        let id = Uuid::new_v4();
        let created_at = Utc::now();
        app.db
            .execute(
                "INSERT INTO branch_protection_rules (id, repo_id, branch_pattern, require_reviews_count, require_status_checks, block_force_push, created_at) \
                 VALUES (?1, ?2, ?3, ?4, 0, ?5, ?6)",
                params!(
                    id.to_string(),
                    repo_id.to_string(),
                    branch_pattern.clone(),
                    require_reviews_count,
                    block_force_push,
                    created_at.to_rfc3339()
                ),
            )
            .await?;

        let rule = entity::branch_protection_rule::Model {
            id,
            repo_id,
            branch_pattern,
            require_reviews_count,
            require_status_checks: false,
            block_force_push,
            created_at,
        };

        Ok(BranchProtectionRuleObject::from(rule))
    }

    /// Submits a review on a pull request (approve / request changes /
    /// comment), recording a `pr_reviews` row. This is the row that
    /// `addReviewComment` attaches inline comments to, and that
    /// `mergePullRequest` counts against branch protection rules.
    async fn submit_pull_request_review(
        &self,
        ctx: &Context<'_>,
        pr_id: Uuid,
        state: String,
        body: Option<String>,
    ) -> async_graphql::Result<PrReviewObject> {
        let app = ctx.data::<AppContext>()?;
        let req = ctx.data::<RequestContext>()?;
        let claims = require_user(req)?;

        let pr = app
            .db
            .query_as::<entity::pull_request::Model, _>("SELECT * FROM pull_requests WHERE id = ?1", params!(pr_id.to_string()))
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| async_graphql::Error::new("pull request not found"))?;
        let repo = find_repo(app, pr.repo_id).await?;
        let perm = repo_permission(app, &repo, claims.sub).await?;
        if perm.is_none() {
            return Err(async_graphql::Error::new("forbidden"));
        }

        let valid_states = [
            entity::pr_review::state::APPROVED,
            entity::pr_review::state::CHANGES_REQUESTED,
            entity::pr_review::state::COMMENTED,
        ];
        if !valid_states.contains(&state.as_str()) {
            return Err(async_graphql::Error::new(format!(
                "invalid review state '{state}'; expected one of {valid_states:?}"
            )));
        }

        let id = Uuid::new_v4();
        let created_at = Utc::now();
        app.db
            .execute(
                "INSERT INTO pr_reviews (id, pr_id, reviewer_id, state, body, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params!(id.to_string(), pr_id.to_string(), claims.sub.to_string(), state.clone(), body.clone(), created_at.to_rfc3339()),
            )
            .await?;

        let review = entity::pr_review::Model {
            id,
            pr_id,
            reviewer_id: claims.sub,
            state,
            body,
            created_at,
        };

        Ok(PrReviewObject::from(review))
    }

    /// Adds an inline comment on a specific file/line of a pull request
    /// review's diff.
    async fn add_review_comment(
        &self,
        ctx: &Context<'_>,
        review_id: Uuid,
        file_path: String,
        line_number: i32,
        body: String,
    ) -> async_graphql::Result<PrReviewCommentObject> {
        let app = ctx.data::<AppContext>()?;
        let req = ctx.data::<RequestContext>()?;
        let claims = require_user(req)?;

        let review = app
            .db
            .query_as::<entity::pr_review::Model, _>("SELECT * FROM pr_reviews WHERE id = ?1", params!(review_id.to_string()))
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| async_graphql::Error::new("review not found"))?;
        let pr = app
            .db
            .query_as::<entity::pull_request::Model, _>(
                "SELECT * FROM pull_requests WHERE id = ?1",
                params!(review.pr_id.to_string()),
            )
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| async_graphql::Error::new("pull request not found"))?;
        let repo = find_repo(app, pr.repo_id).await?;
        let perm = repo_permission(app, &repo, claims.sub).await?;
        if perm.is_none() {
            return Err(async_graphql::Error::new("forbidden"));
        }

        let id = Uuid::new_v4();
        let created_at = Utc::now();
        app.db
            .execute(
                "INSERT INTO pr_review_comments (id, review_id, file_path, line_number, body, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params!(id.to_string(), review_id.to_string(), file_path.clone(), line_number, body.clone(), created_at.to_rfc3339()),
            )
            .await?;

        let comment = entity::pr_review_comment::Model {
            id,
            review_id,
            file_path,
            line_number,
            body,
            created_at,
        };

        Ok(PrReviewCommentObject::from(comment))
    }

    /// Creates an organization, with the current user as its first (owner) member.
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

        let id = Uuid::new_v4();
        let created_at = Utc::now();
        app.db
            .execute(
                "INSERT INTO organizations (id, name, description, created_at) VALUES (?1, ?2, ?3, ?4)",
                params!(id.to_string(), name.clone(), description.clone(), created_at.to_rfc3339()),
            )
            .await?;

        let org = entity::organization::Model {
            id,
            name,
            description,
            created_at,
        };

        app.db
            .execute(
                "INSERT INTO org_members (org_id, user_id, role) VALUES (?1, ?2, ?3)",
                params!(org.id.to_string(), claims.sub.to_string(), entity::org_member::role::OWNER),
            )
            .await?;

        Ok(OrganizationObject::from(org))
    }

    /// Adds an existing user to an organization with the given role
    /// (`owner`/`admin`/`member`). Requires the caller to be an owner or
    /// admin of the organization.
    async fn add_org_member(
        &self,
        ctx: &Context<'_>,
        org_id: Uuid,
        username: String,
        role: String,
    ) -> async_graphql::Result<bool> {
        let app = ctx.data::<AppContext>()?;
        let req = ctx.data::<RequestContext>()?;
        let claims = require_user(req)?;
        require_org_admin(app, org_id, claims.sub).await?;

        let valid_roles = [
            entity::org_member::role::OWNER,
            entity::org_member::role::ADMIN,
            entity::org_member::role::MEMBER,
        ];
        if !valid_roles.contains(&role.as_str()) {
            return Err(async_graphql::Error::new(format!(
                "invalid role '{role}'; expected one of {valid_roles:?}"
            )));
        }

        let user = app
            .db
            .query_as::<entity::user::Model, _>(
                "SELECT * FROM users WHERE username = ?1",
                params!(username),
            )
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| async_graphql::Error::new("user not found"))?;

        app.db
            .execute(
                "INSERT INTO org_members (org_id, user_id, role) VALUES (?1, ?2, ?3)",
                params!(org_id.to_string(), user.id.to_string(), role),
            )
            .await?;
        Ok(true)
    }

    /// Changes an existing organization member's role. Requires the caller
    /// to be an owner or admin of the organization.
    async fn update_org_member_role(
        &self,
        ctx: &Context<'_>,
        org_id: Uuid,
        user_id: Uuid,
        role: String,
    ) -> async_graphql::Result<bool> {
        let app = ctx.data::<AppContext>()?;
        let req = ctx.data::<RequestContext>()?;
        let claims = require_user(req)?;
        require_org_admin(app, org_id, claims.sub).await?;

        let valid_roles = [
            entity::org_member::role::OWNER,
            entity::org_member::role::ADMIN,
            entity::org_member::role::MEMBER,
        ];
        if !valid_roles.contains(&role.as_str()) {
            return Err(async_graphql::Error::new(format!(
                "invalid role '{role}'; expected one of {valid_roles:?}"
            )));
        }

        let existing = app
            .db
            .query_as::<entity::org_member::Model, _>(
                "SELECT * FROM org_members WHERE org_id = ?1 AND user_id = ?2",
                params!(org_id.to_string(), user_id.to_string()),
            )
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| async_graphql::Error::new("member not found"))?;
        let _ = existing;

        app.db
            .execute(
                "UPDATE org_members SET role = ?1 WHERE org_id = ?2 AND user_id = ?3",
                params!(role, org_id.to_string(), user_id.to_string()),
            )
            .await?;
        Ok(true)
    }

    /// Removes a member from an organization. Requires the caller to be an
    /// owner or admin of the organization.
    async fn remove_org_member(
        &self,
        ctx: &Context<'_>,
        org_id: Uuid,
        user_id: Uuid,
    ) -> async_graphql::Result<bool> {
        let app = ctx.data::<AppContext>()?;
        let req = ctx.data::<RequestContext>()?;
        let claims = require_user(req)?;
        require_org_admin(app, org_id, claims.sub).await?;

        let affected = app
            .db
            .execute(
                "DELETE FROM org_members WHERE org_id = ?1 AND user_id = ?2",
                params!(org_id.to_string(), user_id.to_string()),
            )
            .await?;
        Ok(affected > 0)
    }

    /// Deletes an organization (and, via foreign-key cascade at the DB
    /// level if configured, its memberships). Note this does *not* delete
    /// repositories owned by the organization; those remain with a dangling
    /// `owner_id` and should be reassigned or deleted first. Requires the
    /// caller to be an owner of the organization.
    async fn delete_organization(&self, ctx: &Context<'_>, org_id: Uuid) -> async_graphql::Result<bool> {
        let app = ctx.data::<AppContext>()?;
        let req = ctx.data::<RequestContext>()?;
        let claims = require_user(req)?;

        let is_owner = app
            .db
            .query_as::<entity::org_member::Model, _>(
                "SELECT * FROM org_members WHERE org_id = ?1 AND user_id = ?2",
                params!(org_id.to_string(), claims.sub.to_string()),
            )
            .await?
            .into_iter()
            .next()
            .map(|m| m.role == entity::org_member::role::OWNER)
            .unwrap_or(false);
        if !is_owner {
            return Err(async_graphql::Error::new("forbidden"));
        }

        app.db
            .execute("DELETE FROM org_members WHERE org_id = ?1", params!(org_id.to_string()))
            .await?;
        app.db
            .execute("DELETE FROM organizations WHERE id = ?1", params!(org_id.to_string()))
            .await?;
        Ok(true)
    }

    /// Grants a user a permission level (`read`/`write`/`admin`) on a repository.
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

        let collaborator = app
            .db
            .query_as::<entity::user::Model, _>(
                "SELECT * FROM users WHERE username = ?1",
                params!(username),
            )
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| async_graphql::Error::new("user not found"))?;

        app.db
            .execute(
                "INSERT INTO repo_collaborators (repo_id, user_id, permission) VALUES (?1, ?2, ?3)",
                params!(repo_id.to_string(), collaborator.id.to_string(), permission),
            )
            .await?;

        Ok(true)
    }

    /// Removes a collaborator's explicit access grant from a repository.
    /// Requires Admin permission. Does not affect access derived from
    /// ownership or organization role.
    async fn remove_collaborator(
        &self,
        ctx: &Context<'_>,
        repo_id: Uuid,
        user_id: Uuid,
    ) -> async_graphql::Result<bool> {
        let app = ctx.data::<AppContext>()?;
        let req = ctx.data::<RequestContext>()?;
        let claims = require_user(req)?;

        let repo = find_repo(app, repo_id).await?;
        let perm = repo_permission(app, &repo, claims.sub).await?;
        if perm != Some(Permission::Admin) {
            return Err(async_graphql::Error::new("forbidden"));
        }

        let affected = app
            .db
            .execute(
                "DELETE FROM repo_collaborators WHERE repo_id = ?1 AND user_id = ?2",
                params!(repo_id.to_string(), user_id.to_string()),
            )
            .await?;
        Ok(affected > 0)
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

        let id = Uuid::new_v4();
        let workflow_name = workflow.name.clone().unwrap_or_else(|| workflow_path.clone());
        let event = "workflow_dispatch".to_string();
        let status = entity::workflow_run::status::QUEUED.to_string();
        app.db
            .execute(
                "INSERT INTO workflow_runs (id, repo_id, workflow_name, commit_sha, event, status, started_at, finished_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL, NULL)",
                params!(
                    id.to_string(),
                    repo_id.to_string(),
                    workflow_name.clone(),
                    commit_sha.clone(),
                    event.clone(),
                    status.clone()
                ),
            )
            .await?;

        let run = entity::workflow_run::Model {
            id,
            repo_id,
            workflow_name,
            commit_sha,
            event,
            status,
            started_at: None,
            finished_at: None,
        };

        let secrets = load_repo_secrets(app, repo_id).await.unwrap_or_default();

        // Record each job in `runner_jobs` for history/observability, with
        // status `HANDLED_INPROCESS` (not `QUEUED`) since the in-process
        // `tokio::spawn` + `executor.run_job` call below is about to run it
        // -- `/runner/claim` only selects `QUEUED` rows, so this can never
        // be double-executed by a connected standalone runner.
        for job in workflow.jobs.values() {
            let payload = serde_json::json!({
                "run_id": run.id,
                "job": job,
                "repo_archive_b64": "",
                "env_extra": serde_json::Map::<String, serde_json::Value>::new(),
                "secrets": secrets,
            })
            .to_string();
            let res = app
                .db
                .execute(
                    "INSERT INTO runner_jobs (id, kind, repo_id, workflow_run_id, payload, status, claimed_by, claimed_at, created_at, finished_at) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL, NULL, ?7, NULL)",
                    params!(
                        Uuid::new_v4().to_string(),
                        entity::runner_job::kind::CI_JOB.to_string(),
                        Some(repo_id.to_string()),
                        Some(run.id.to_string()),
                        payload,
                        entity::runner_job::status::HANDLED_INPROCESS.to_string(),
                        Utc::now().to_rfc3339()
                    ),
                )
                .await;
            if let Err(e) = res {
                tracing::warn!("failed to record runner_job history row (non-fatal): {e}");
            }
        }

        // Spawn execution of every job in the background; this is fire-and-forget
        // since GraphQL mutations should return promptly. Job/run status updates
        // would normally be persisted by a background task watching JobResults.
        let executor = app.actions_executor.clone();
        let jobs: Vec<_> = workflow.jobs.into_values().collect();
        let run_id = run.id;
        let db = app.db.clone();
        tokio::spawn(async move {
            for job in jobs {
                let result = executor
                    .run_job(run_id, &job, &[], Default::default(), &secrets, |line| {
                        tracing::info!(target: "workflow", "{line}");
                    })
                    .await;
                match result {
                    Ok(job_result) => {
                        for artifact in &job_result.artifacts {
                            let artifact_id = Uuid::new_v4();
                            let created_at = Utc::now();
                            let res = db
                                .execute(
                                    "INSERT INTO workflow_artifacts (id, run_id, job_id, name, file_path, size_bytes, created_at) \
                                     VALUES (?1, ?2, NULL, ?3, ?4, ?5, ?6)",
                                    params!(
                                        artifact_id.to_string(),
                                        run_id.to_string(),
                                        artifact.name.clone(),
                                        artifact.file_path.clone(),
                                        artifact.size_bytes,
                                        created_at.to_rfc3339()
                                    ),
                                )
                                .await;
                            if let Err(e) = res {
                                tracing::warn!("failed to insert workflow_artifact: {e}");
                            }
                        }
                    }
                    Err(e) => {
                        tracing::warn!("workflow job failed: {e}");
                    }
                }
            }
        });

        Ok(WorkflowRunObject::from(run))
    }

    /// Creates a dev workspace either from a named built-in template
    /// (`template`, e.g. `"code-server"`/`"rust-dev"`/`"node-dev"`) or from a
    /// raw `image` for advanced/custom use. `template` takes precedence if
    /// both are given; a raw `image` with no `template` falls back to the
    /// single-port code-server-only behavior that predates templates.
    ///
    /// If `on_runner` is true, instead of creating the container directly
    /// against `server`'s own Docker daemon, this enqueues a
    /// `dev_workspace_action` job (`kind::DEV_WORKSPACE_ACTION`) for a
    /// connected standalone `runner` process to claim and create the
    /// container against its *own* Docker daemon (see
    /// `runner/src/dev_workspace_poll.rs`). The returned workspace has
    /// `status: "pending_runner"` and a `null` `containerId`/`runnerId`
    /// until a runner claims the job (poll `devWorkspace(id)` for the
    /// updated status). Errors if no runner claims it within 20s.
    ///
    /// Known gap: live port-proxying (`GET /workspaces/:id/proxy/*path`)
    /// only works for server-hosted workspaces today -- a runner-hosted
    /// workspace's container is on a different, not-necessarily-reachable
    /// Docker host, and there is no reverse-tunnel between the runner and
    /// `server` yet to route proxied HTTP requests to it.
    async fn create_dev_workspace(
        &self,
        ctx: &Context<'_>,
        name: String,
        template: Option<String>,
        image: Option<String>,
        repo_id: Option<Uuid>,
        auto_stop_minutes: Option<i32>,
        on_runner: Option<bool>,
    ) -> async_graphql::Result<DevWorkspaceObject> {
        let app = ctx.data::<AppContext>()?;
        let req = ctx.data::<RequestContext>()?;
        let claims = require_user(req)?;

        let (image, ports): (String, &[(&str, u16)]) = if let Some(template_name) = template.as_deref() {
            let template = dev_env::find_template(template_name).ok_or_else(|| {
                async_graphql::Error::new(format!("unknown workspace template '{template_name}'"))
            })?;
            (template.image.to_string(), template.ports)
        } else {
            (
                image.unwrap_or_else(|| dev_env::manager::IMAGE_CODE_SERVER.to_string()),
                &[],
            )
        };

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

        let id = Uuid::new_v4();
        let created_at = Utc::now();

        if on_runner.unwrap_or(false) {
            let payload = serde_json::json!({
                "workspace_id": id,
                "name": name,
                "image": image,
                "ports": ports.iter().map(|(n, p)| (n.to_string(), *p)).collect::<Vec<_>>(),
                "repo_clone_url": repo_clone_url,
                "owner": claims.username,
            });
            let job_id = enqueue_dev_workspace_job(app, "create", &payload).await?;

            let status = entity::dev_workspace::status::PENDING_RUNNER.to_string();
            app.db
                .execute(
                    "INSERT INTO dev_workspaces (id, owner_id, repo_id, name, image, status, container_id, created_at, auto_stop_minutes, last_activity_at, runner_id) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL, ?7, ?8, NULL, NULL)",
                    params!(
                        id.to_string(),
                        claims.sub.to_string(),
                        repo_id.map(|r| r.to_string()),
                        name.clone(),
                        image.clone(),
                        status.clone(),
                        created_at.to_rfc3339(),
                        auto_stop_minutes
                    ),
                )
                .await?;

            let job = wait_for_runner_job(app, job_id, std::time::Duration::from_secs(20)).await?;
            let (final_status, container_id, runner_id) = apply_create_result(&job);
            app.db
                .execute(
                    "UPDATE dev_workspaces SET status = ?1, container_id = ?2, runner_id = ?3 WHERE id = ?4",
                    params!(final_status.clone(), container_id.clone(), runner_id.clone(), id.to_string()),
                )
                .await?;

            let workspace = entity::dev_workspace::Model {
                id,
                owner_id: claims.sub,
                repo_id,
                name,
                image,
                status: final_status,
                container_id,
                created_at,
                auto_stop_minutes,
                last_activity_at: None,
                runner_id,
            };
            return Ok(DevWorkspaceObject::from(workspace));
        }

        let handle = app
            .workspace_manager
            .create_workspace(
                &name,
                &image,
                ports,
                repo_clone_url.as_deref(),
                None,
                None,
                &claims.username,
            )
            .await
            .map_err(|e| async_graphql::Error::new(e.to_string()))?;

        let status = entity::dev_workspace::status::RUNNING.to_string();
        let container_id = Some(handle.container_id);
        let last_activity_at = Some(Utc::now());
        app.db
            .execute(
                "INSERT INTO dev_workspaces (id, owner_id, repo_id, name, image, status, container_id, created_at, auto_stop_minutes, last_activity_at, runner_id) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, NULL)",
                params!(
                    id.to_string(),
                    claims.sub.to_string(),
                    repo_id.map(|r| r.to_string()),
                    name.clone(),
                    image.clone(),
                    status.clone(),
                    container_id.clone(),
                    created_at.to_rfc3339(),
                    auto_stop_minutes,
                    last_activity_at.map(|t: chrono::DateTime<Utc>| t.to_rfc3339())
                ),
            )
            .await?;

        let workspace = entity::dev_workspace::Model {
            id,
            owner_id: claims.sub,
            repo_id,
            name,
            image,
            status,
            container_id,
            created_at,
            auto_stop_minutes,
            last_activity_at,
            runner_id: None,
        };
        Ok(DevWorkspaceObject::from(workspace))
    }

    /// Starts a stopped dev workspace's container.
    async fn start_dev_workspace(&self, ctx: &Context<'_>, workspace_id: Uuid) -> async_graphql::Result<DevWorkspaceObject> {
        let app = ctx.data::<AppContext>()?;
        let req = ctx.data::<RequestContext>()?;
        let claims = require_user(req)?;

        let mut workspace = app
            .db
            .query_as::<entity::dev_workspace::Model, _>(
                "SELECT * FROM dev_workspaces WHERE id = ?1",
                params!(workspace_id.to_string()),
            )
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| async_graphql::Error::new("workspace not found"))?;
        if workspace.owner_id != claims.sub {
            return Err(async_graphql::Error::new("forbidden"));
        }
        if workspace.runner_id.is_some() {
            return Err(async_graphql::Error::new(
                "starting/stopping a runner-hosted dev workspace is not supported yet -- delete and recreate it instead",
            ));
        }
        let container_id = workspace
            .container_id
            .clone()
            .ok_or_else(|| async_graphql::Error::new("workspace has no container"))?;

        app.workspace_manager
            .start_workspace(&container_id)
            .await
            .map_err(|e| async_graphql::Error::new(e.to_string()))?;

        let status = entity::dev_workspace::status::RUNNING.to_string();
        app.db
            .execute(
                "UPDATE dev_workspaces SET status = ?1 WHERE id = ?2",
                params!(status.clone(), workspace_id.to_string()),
            )
            .await?;
        workspace.status = status;
        Ok(DevWorkspaceObject::from(workspace))
    }

    /// Stops a running dev workspace's container (keeps its filesystem).
    async fn stop_dev_workspace(&self, ctx: &Context<'_>, workspace_id: Uuid) -> async_graphql::Result<DevWorkspaceObject> {
        let app = ctx.data::<AppContext>()?;
        let req = ctx.data::<RequestContext>()?;
        let claims = require_user(req)?;

        let mut workspace = app
            .db
            .query_as::<entity::dev_workspace::Model, _>(
                "SELECT * FROM dev_workspaces WHERE id = ?1",
                params!(workspace_id.to_string()),
            )
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| async_graphql::Error::new("workspace not found"))?;
        if workspace.owner_id != claims.sub {
            return Err(async_graphql::Error::new("forbidden"));
        }
        if workspace.runner_id.is_some() {
            return Err(async_graphql::Error::new(
                "starting/stopping a runner-hosted dev workspace is not supported yet -- delete and recreate it instead",
            ));
        }
        let container_id = workspace
            .container_id
            .clone()
            .ok_or_else(|| async_graphql::Error::new("workspace has no container"))?;

        app.workspace_manager
            .stop_workspace(&container_id)
            .await
            .map_err(|e| async_graphql::Error::new(e.to_string()))?;

        let status = entity::dev_workspace::status::STOPPED.to_string();
        app.db
            .execute(
                "UPDATE dev_workspaces SET status = ?1 WHERE id = ?2",
                params!(status.clone(), workspace_id.to_string()),
            )
            .await?;
        workspace.status = status;
        Ok(DevWorkspaceObject::from(workspace))
    }

    /// Deletes a dev workspace and its container/volumes.
    async fn delete_dev_workspace(&self, ctx: &Context<'_>, workspace_id: Uuid) -> async_graphql::Result<bool> {
        let app = ctx.data::<AppContext>()?;
        let req = ctx.data::<RequestContext>()?;
        let claims = require_user(req)?;

        let workspace = app
            .db
            .query_as::<entity::dev_workspace::Model, _>(
                "SELECT * FROM dev_workspaces WHERE id = ?1",
                params!(workspace_id.to_string()),
            )
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| async_graphql::Error::new("workspace not found"))?;
        if workspace.owner_id != claims.sub {
            return Err(async_graphql::Error::new("forbidden"));
        }

        if workspace.runner_id.is_some() {
            if let Some(container_id) = &workspace.container_id {
                let payload = serde_json::json!({
                    "workspace_id": workspace.id,
                    "container_id": container_id,
                });
                let job_id = enqueue_dev_workspace_job(app, "delete", &payload).await?;
                // Best-effort: the DB row is removed below regardless of
                // whether the runner confirms deletion in time, so a slow
                // or disconnected runner doesn't strand the user with an
                // undeletable workspace record -- it may leave an orphaned
                // container on the runner's host if the job never lands.
                let _ = wait_for_runner_job(app, job_id, std::time::Duration::from_secs(20)).await;
            }
        } else if let Some(container_id) = &workspace.container_id {
            app.workspace_manager
                .delete_workspace(container_id)
                .await
                .map_err(|e| async_graphql::Error::new(e.to_string()))?;
        }

        app.db
            .execute("DELETE FROM dev_workspaces WHERE id = ?1", params!(workspace.id.to_string()))
            .await?;
        Ok(true)
    }

    // ---- Labels ----

    /// Creates a label (name + hex color) for a repository, usable on issues.
    async fn create_label(
        &self,
        ctx: &Context<'_>,
        repo_id: Uuid,
        name: String,
        color: String,
    ) -> async_graphql::Result<LabelObject> {
        let app = ctx.data::<AppContext>()?;
        let req = ctx.data::<RequestContext>()?;
        let claims = require_user(req)?;

        let repo = find_repo(app, repo_id).await?;
        let perm = repo_permission(app, &repo, claims.sub).await?;
        if perm < Some(Permission::Write) {
            return Err(async_graphql::Error::new("forbidden"));
        }

        let id = Uuid::new_v4();
        app.db
            .execute(
                "INSERT INTO labels (id, repo_id, name, color) VALUES (?1, ?2, ?3, ?4)",
                params!(id.to_string(), repo_id.to_string(), name.clone(), color.clone()),
            )
            .await?;

        let label = entity::label::Model {
            id,
            repo_id,
            name,
            color,
        };
        Ok(LabelObject::from(label))
    }

    /// Attaches an existing label to an issue.
    async fn add_label_to_issue(
        &self,
        ctx: &Context<'_>,
        issue_id: Uuid,
        label_id: Uuid,
    ) -> async_graphql::Result<IssueObject> {
        let app = ctx.data::<AppContext>()?;
        let req = ctx.data::<RequestContext>()?;
        let claims = require_user(req)?;

        let issue = app
            .db
            .query_as::<entity::issue::Model, _>("SELECT * FROM issues WHERE id = ?1", params!(issue_id.to_string()))
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| async_graphql::Error::new("issue not found"))?;
        let repo = find_repo(app, issue.repo_id).await?;
        let perm = repo_permission(app, &repo, claims.sub).await?;
        if perm < Some(Permission::Write) {
            return Err(async_graphql::Error::new("forbidden"));
        }

        let label = app
            .db
            .query_as::<entity::label::Model, _>("SELECT * FROM labels WHERE id = ?1", params!(label_id.to_string()))
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| async_graphql::Error::new("label not found"))?;
        if label.repo_id != repo.id {
            return Err(async_graphql::Error::new("label does not belong to this repository"));
        }

        let existing = app
            .db
            .query_as::<entity::issue_label::Model, _>(
                "SELECT * FROM issue_labels WHERE issue_id = ?1 AND label_id = ?2",
                params!(issue_id.to_string(), label_id.to_string()),
            )
            .await?
            .into_iter()
            .next();
        if existing.is_none() {
            app.db
                .execute(
                    "INSERT INTO issue_labels (issue_id, label_id) VALUES (?1, ?2)",
                    params!(issue_id.to_string(), label_id.to_string()),
                )
                .await?;
        }

        Ok(IssueObject::from(issue))
    }

    /// Detaches a label from an issue.
    async fn remove_label_from_issue(
        &self,
        ctx: &Context<'_>,
        issue_id: Uuid,
        label_id: Uuid,
    ) -> async_graphql::Result<IssueObject> {
        let app = ctx.data::<AppContext>()?;
        let req = ctx.data::<RequestContext>()?;
        let claims = require_user(req)?;

        let issue = app
            .db
            .query_as::<entity::issue::Model, _>("SELECT * FROM issues WHERE id = ?1", params!(issue_id.to_string()))
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| async_graphql::Error::new("issue not found"))?;
        let repo = find_repo(app, issue.repo_id).await?;
        let perm = repo_permission(app, &repo, claims.sub).await?;
        if perm < Some(Permission::Write) {
            return Err(async_graphql::Error::new("forbidden"));
        }

        app.db
            .execute(
                "DELETE FROM issue_labels WHERE issue_id = ?1 AND label_id = ?2",
                params!(issue_id.to_string(), label_id.to_string()),
            )
            .await?;

        Ok(IssueObject::from(issue))
    }

    // ---- Milestones ----

    /// Creates a milestone (optional due date) for a repository.
    async fn create_milestone(
        &self,
        ctx: &Context<'_>,
        repo_id: Uuid,
        title: String,
        description: Option<String>,
        due_date: Option<chrono::DateTime<Utc>>,
    ) -> async_graphql::Result<MilestoneObject> {
        let app = ctx.data::<AppContext>()?;
        let req = ctx.data::<RequestContext>()?;
        let claims = require_user(req)?;

        let repo = find_repo(app, repo_id).await?;
        let perm = repo_permission(app, &repo, claims.sub).await?;
        if perm < Some(Permission::Write) {
            return Err(async_graphql::Error::new("forbidden"));
        }

        let id = Uuid::new_v4();
        let created_at = Utc::now();
        let state = entity::milestone::state::OPEN.to_string();
        app.db
            .execute(
                "INSERT INTO milestones (id, repo_id, title, description, due_date, state, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params!(
                    id.to_string(),
                    repo_id.to_string(),
                    title.clone(),
                    description.clone(),
                    due_date.map(|d| d.to_rfc3339()),
                    state.clone(),
                    created_at.to_rfc3339()
                ),
            )
            .await?;

        let milestone = entity::milestone::Model {
            id,
            repo_id,
            title,
            description,
            due_date,
            state,
            created_at,
        };
        Ok(MilestoneObject::from(milestone))
    }

    /// Sets (or clears, with `null`) an issue's milestone.
    async fn set_issue_milestone(
        &self,
        ctx: &Context<'_>,
        issue_id: Uuid,
        milestone_id: Option<Uuid>,
    ) -> async_graphql::Result<IssueObject> {
        let app = ctx.data::<AppContext>()?;
        let req = ctx.data::<RequestContext>()?;
        let claims = require_user(req)?;

        let mut issue = app
            .db
            .query_as::<entity::issue::Model, _>("SELECT * FROM issues WHERE id = ?1", params!(issue_id.to_string()))
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| async_graphql::Error::new("issue not found"))?;
        let repo = find_repo(app, issue.repo_id).await?;
        let perm = repo_permission(app, &repo, claims.sub).await?;
        if perm < Some(Permission::Write) {
            return Err(async_graphql::Error::new("forbidden"));
        }

        if let Some(mid) = milestone_id {
            let milestone = app
                .db
                .query_as::<entity::milestone::Model, _>("SELECT * FROM milestones WHERE id = ?1", params!(mid.to_string()))
                .await?
                .into_iter()
                .next()
                .ok_or_else(|| async_graphql::Error::new("milestone not found"))?;
            if milestone.repo_id != repo.id {
                return Err(async_graphql::Error::new(
                    "milestone does not belong to this repository",
                ));
            }
        }

        app.db
            .execute(
                "UPDATE issues SET milestone_id = ?1 WHERE id = ?2",
                params!(milestone_id.map(|m| m.to_string()), issue_id.to_string()),
            )
            .await?;
        issue.milestone_id = milestone_id;

        Ok(IssueObject::from(issue))
    }

    // ---- Projects ----

    /// Creates a Kanban project (board) for a repository.
    async fn create_project(
        &self,
        ctx: &Context<'_>,
        repo_id: Uuid,
        name: String,
    ) -> async_graphql::Result<ProjectObject> {
        let app = ctx.data::<AppContext>()?;
        let req = ctx.data::<RequestContext>()?;
        let claims = require_user(req)?;

        let repo = find_repo(app, repo_id).await?;
        let perm = repo_permission(app, &repo, claims.sub).await?;
        if perm < Some(Permission::Write) {
            return Err(async_graphql::Error::new("forbidden"));
        }

        let id = Uuid::new_v4();
        let created_at = Utc::now();
        app.db
            .execute(
                "INSERT INTO projects (id, repo_id, name, created_at) VALUES (?1, ?2, ?3, ?4)",
                params!(id.to_string(), repo_id.to_string(), name.clone(), created_at.to_rfc3339()),
            )
            .await?;

        let project = entity::project::Model {
            id,
            repo_id,
            name,
            created_at,
        };
        Ok(ProjectObject::from(project))
    }

    /// Adds a column (e.g. "To do"/"Done") to a project, at the given position.
    async fn add_project_column(
        &self,
        ctx: &Context<'_>,
        project_id: Uuid,
        name: String,
        position: i32,
    ) -> async_graphql::Result<ProjectColumnObject> {
        let app = ctx.data::<AppContext>()?;
        let req = ctx.data::<RequestContext>()?;
        let claims = require_user(req)?;

        let project = app
            .db
            .query_as::<entity::project::Model, _>("SELECT * FROM projects WHERE id = ?1", params!(project_id.to_string()))
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| async_graphql::Error::new("project not found"))?;
        let repo = find_repo(app, project.repo_id).await?;
        let perm = repo_permission(app, &repo, claims.sub).await?;
        if perm < Some(Permission::Write) {
            return Err(async_graphql::Error::new("forbidden"));
        }

        let id = Uuid::new_v4();
        app.db
            .execute(
                "INSERT INTO project_columns (id, project_id, name, position) VALUES (?1, ?2, ?3, ?4)",
                params!(id.to_string(), project_id.to_string(), name.clone(), position),
            )
            .await?;

        let column = entity::project_column::Model {
            id,
            project_id,
            name,
            position,
        };
        Ok(ProjectColumnObject::from(column))
    }

    /// Adds an issue as a card in a project column, at the given position.
    async fn add_card_to_column(
        &self,
        ctx: &Context<'_>,
        column_id: Uuid,
        issue_id: Uuid,
        position: i32,
    ) -> async_graphql::Result<ProjectCardObject> {
        let app = ctx.data::<AppContext>()?;
        let req = ctx.data::<RequestContext>()?;
        let claims = require_user(req)?;

        let column = app
            .db
            .query_as::<entity::project_column::Model, _>(
                "SELECT * FROM project_columns WHERE id = ?1",
                params!(column_id.to_string()),
            )
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| async_graphql::Error::new("project column not found"))?;
        let project = app
            .db
            .query_as::<entity::project::Model, _>(
                "SELECT * FROM projects WHERE id = ?1",
                params!(column.project_id.to_string()),
            )
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| async_graphql::Error::new("project not found"))?;
        let repo = find_repo(app, project.repo_id).await?;
        let perm = repo_permission(app, &repo, claims.sub).await?;
        if perm < Some(Permission::Write) {
            return Err(async_graphql::Error::new("forbidden"));
        }

        let issue = app
            .db
            .query_as::<entity::issue::Model, _>("SELECT * FROM issues WHERE id = ?1", params!(issue_id.to_string()))
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| async_graphql::Error::new("issue not found"))?;
        if issue.repo_id != repo.id {
            return Err(async_graphql::Error::new("issue does not belong to this repository"));
        }

        let id = Uuid::new_v4();
        app.db
            .execute(
                "INSERT INTO project_cards (id, column_id, issue_id, pull_request_id, position) VALUES (?1, ?2, ?3, NULL, ?4)",
                params!(id.to_string(), column_id.to_string(), issue_id.to_string(), position),
            )
            .await?;

        let card = entity::project_card::Model {
            id,
            column_id,
            issue_id: Some(issue_id),
            pull_request_id: None,
            position,
        };
        Ok(ProjectCardObject::from(card))
    }

    // ---- Notifications ----

    /// Marks a notification as read. Only the notification's owner may do this.
    async fn mark_notification_read(&self, ctx: &Context<'_>, id: Uuid) -> async_graphql::Result<NotificationObject> {
        let app = ctx.data::<AppContext>()?;
        let req = ctx.data::<RequestContext>()?;
        let claims = require_user(req)?;

        let mut notification = app
            .db
            .query_as::<entity::notification::Model, _>(
                "SELECT * FROM notifications WHERE id = ?1",
                params!(id.to_string()),
            )
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| async_graphql::Error::new("notification not found"))?;
        if notification.user_id != claims.sub {
            return Err(async_graphql::Error::new("forbidden"));
        }

        let read_at = Utc::now();
        app.db
            .execute(
                "UPDATE notifications SET read_at = ?1 WHERE id = ?2",
                params!(read_at.to_rfc3339(), id.to_string()),
            )
            .await?;
        notification.read_at = Some(read_at);

        Ok(NotificationObject::from(notification))
    }

    /// Sets (creates or overwrites) an encrypted Actions secret for a
    /// repository. Requires Admin permission on the repository. Secrets are
    /// write-only: there is no query to read the value back, only
    /// `repository.secretNames` to list which names exist.
    async fn set_repo_secret(
        &self,
        ctx: &Context<'_>,
        repo_id: Uuid,
        name: String,
        value: String,
    ) -> async_graphql::Result<bool> {
        let app = ctx.data::<AppContext>()?;
        let req = ctx.data::<RequestContext>()?;
        let claims = require_user(req)?;

        let repo = find_repo(app, repo_id).await?;
        let perm = repo_permission(app, &repo, claims.sub).await?;
        if perm != Some(Permission::Admin) {
            return Err(async_graphql::Error::new("forbidden"));
        }

        let key = secrets_encryption_key().map_err(|e| async_graphql::Error::new(e.to_string()))?;
        let encrypted_value = actions::encrypt_secret(&key, &value);

        let existing = app
            .db
            .query_as::<entity::repo_secret::Model, _>(
                "SELECT * FROM repo_secrets WHERE repo_id = ?1 AND name = ?2",
                params!(repo_id.to_string(), name.clone()),
            )
            .await?
            .into_iter()
            .next();

        if let Some(existing) = existing {
            app.db
                .execute(
                    "UPDATE repo_secrets SET encrypted_value = ?1 WHERE id = ?2",
                    params!(encrypted_value, existing.id.to_string()),
                )
                .await?;
        } else {
            let id = Uuid::new_v4();
            let created_at = Utc::now();
            app.db
                .execute(
                    "INSERT INTO repo_secrets (id, repo_id, name, encrypted_value, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
                    params!(id.to_string(), repo_id.to_string(), name, encrypted_value, created_at.to_rfc3339()),
                )
                .await?;
        }

        Ok(true)
    }

    /// Creates or updates the mirror configuration for a repository: a
    /// background task (see `crates/server/src/main.rs`) periodically fetches
    /// from `remote_url` into the repository's bare git directory. Requires
    /// Admin permission on the repository.
    async fn set_repo_mirror(
        &self,
        ctx: &Context<'_>,
        repo_id: Uuid,
        remote_url: String,
        sync_interval_minutes: Option<i32>,
    ) -> async_graphql::Result<bool> {
        let app = ctx.data::<AppContext>()?;
        let req = ctx.data::<RequestContext>()?;
        let claims = require_user(req)?;

        let repo = find_repo(app, repo_id).await?;
        let perm = repo_permission(app, &repo, claims.sub).await?;
        if perm != Some(Permission::Admin) {
            return Err(async_graphql::Error::new("forbidden"));
        }

        let sync_interval_minutes = sync_interval_minutes.unwrap_or(60);

        let existing = app
            .db
            .query_as::<entity::repo_mirror::Model, _>(
                "SELECT * FROM repo_mirrors WHERE repo_id = ?1",
                params!(repo_id.to_string()),
            )
            .await?
            .into_iter()
            .next();

        if let Some(existing) = existing {
            app.db
                .execute(
                    "UPDATE repo_mirrors SET remote_url = ?1, sync_interval_minutes = ?2 WHERE id = ?3",
                    params!(remote_url, sync_interval_minutes, existing.id.to_string()),
                )
                .await?;
        } else {
            let id = Uuid::new_v4();
            let created_at = Utc::now();
            app.db
                .execute(
                    "INSERT INTO repo_mirrors (id, repo_id, remote_url, last_synced_at, sync_interval_minutes, created_at) \
                     VALUES (?1, ?2, ?3, NULL, ?4, ?5)",
                    params!(id.to_string(), repo_id.to_string(), remote_url, sync_interval_minutes, created_at.to_rfc3339()),
                )
                .await?;
        }

        Ok(true)
    }

    // ---- Personal access tokens ----

    /// Creates a new personal access token (PAT) for the current user,
    /// usable as `Authorization: token <value>` against the GraphQL API,
    /// git smart-HTTP, and REST routes (an alternative to logging in and
    /// getting a short-lived JWT — the primary auth mechanism for
    /// API-only/automation use). The plaintext `token` is returned exactly
    /// once; only its SHA256 hash is persisted.
    async fn create_access_token(
        &self,
        ctx: &Context<'_>,
        name: String,
        #[graphql(default)] scopes: Vec<String>,
        expires_in_days: Option<i64>,
    ) -> async_graphql::Result<AccessTokenCreated> {
        let app = ctx.data::<AppContext>()?;
        let req = ctx.data::<RequestContext>()?;
        let claims = require_user(req)?;

        let (token, token_hash) = auth::generate_access_token();
        let expires_at = expires_in_days.map(|days| Utc::now() + chrono::Duration::days(days));

        let id = Uuid::new_v4();
        let scopes_json = serde_json::to_string(&scopes).unwrap_or_else(|_| "[]".to_string());
        app.db
            .execute(
                "INSERT INTO access_tokens (id, user_id, token_hash, name, scopes, expires_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params!(
                    id.to_string(),
                    claims.sub.to_string(),
                    token_hash.clone(),
                    name.clone(),
                    scopes_json.clone(),
                    expires_at.map(|d| d.to_rfc3339())
                ),
            )
            .await?;

        let row = entity::access_token::Model {
            id,
            user_id: claims.sub,
            token_hash,
            name,
            scopes: scopes_json,
            expires_at,
        };

        Ok(AccessTokenCreated {
            token,
            access_token: AccessTokenObject::from(row),
        })
    }

    /// Revokes (deletes) one of the current user's personal access tokens.
    async fn revoke_access_token(&self, ctx: &Context<'_>, id: Uuid) -> async_graphql::Result<bool> {
        let app = ctx.data::<AppContext>()?;
        let req = ctx.data::<RequestContext>()?;
        let claims = require_user(req)?;

        let affected = app
            .db
            .execute(
                "DELETE FROM access_tokens WHERE id = ?1 AND user_id = ?2",
                params!(id.to_string(), claims.sub.to_string()),
            )
            .await?;
        Ok(affected > 0)
    }

    // ---- Webhooks ----

    /// Registers a webhook: `target_url` receives a signed POST for each
    /// event in `events` (e.g. `"issues"`, `"pull_request"`, `"push"`,
    /// `"issue_comment"`) that occurs on the repository. `secret` is used by
    /// the receiver to verify the payload signature (see the `webhooks`
    /// crate's dispatcher). Requires Admin permission on the repository.
    async fn create_webhook(
        &self,
        ctx: &Context<'_>,
        repo_id: Uuid,
        target_url: String,
        secret: String,
        events: Vec<String>,
    ) -> async_graphql::Result<WebhookObject> {
        let app = ctx.data::<AppContext>()?;
        let req = ctx.data::<RequestContext>()?;
        let claims = require_user(req)?;

        let repo = find_repo(app, repo_id).await?;
        let perm = repo_permission(app, &repo, claims.sub).await?;
        if perm != Some(Permission::Admin) {
            return Err(async_graphql::Error::new("forbidden"));
        }

        let id = Uuid::new_v4();
        let events_json = serde_json::to_string(&events).unwrap_or_else(|_| "[]".to_string());
        app.db
            .execute(
                "INSERT INTO webhooks (id, repo_id, target_url, secret, events, active) VALUES (?1, ?2, ?3, ?4, ?5, 1)",
                params!(id.to_string(), repo_id.to_string(), target_url.clone(), secret.clone(), events_json.clone()),
            )
            .await?;

        let webhook = entity::webhook::Model {
            id,
            repo_id,
            target_url,
            secret,
            events: events_json,
            active: true,
        };
        Ok(WebhookObject::from(webhook))
    }

    /// Updates a webhook's target URL, subscribed events, and/or
    /// active/inactive status. Requires Admin permission on the repository.
    async fn update_webhook(
        &self,
        ctx: &Context<'_>,
        webhook_id: Uuid,
        target_url: Option<String>,
        events: Option<Vec<String>>,
        active: Option<bool>,
    ) -> async_graphql::Result<WebhookObject> {
        let app = ctx.data::<AppContext>()?;
        let req = ctx.data::<RequestContext>()?;
        let claims = require_user(req)?;

        let mut webhook = app
            .db
            .query_as::<entity::webhook::Model, _>("SELECT * FROM webhooks WHERE id = ?1", params!(webhook_id.to_string()))
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| async_graphql::Error::new("webhook not found"))?;
        let repo = find_repo(app, webhook.repo_id).await?;
        let perm = repo_permission(app, &repo, claims.sub).await?;
        if perm != Some(Permission::Admin) {
            return Err(async_graphql::Error::new("forbidden"));
        }

        if let Some(target_url) = target_url {
            app.db
                .execute(
                    "UPDATE webhooks SET target_url = ?1 WHERE id = ?2",
                    params!(target_url.clone(), webhook_id.to_string()),
                )
                .await?;
            webhook.target_url = target_url;
        }
        if let Some(events) = events {
            let events_json = serde_json::to_string(&events).unwrap_or_else(|_| "[]".to_string());
            app.db
                .execute(
                    "UPDATE webhooks SET events = ?1 WHERE id = ?2",
                    params!(events_json.clone(), webhook_id.to_string()),
                )
                .await?;
            webhook.events = events_json;
        }
        if let Some(active) = active {
            app.db
                .execute(
                    "UPDATE webhooks SET active = ?1 WHERE id = ?2",
                    params!(active, webhook_id.to_string()),
                )
                .await?;
            webhook.active = active;
        }
        Ok(WebhookObject::from(webhook))
    }

    /// Deletes a webhook. Requires Admin permission on the repository.
    async fn delete_webhook(&self, ctx: &Context<'_>, webhook_id: Uuid) -> async_graphql::Result<bool> {
        let app = ctx.data::<AppContext>()?;
        let req = ctx.data::<RequestContext>()?;
        let claims = require_user(req)?;

        let webhook = app
            .db
            .query_as::<entity::webhook::Model, _>("SELECT * FROM webhooks WHERE id = ?1", params!(webhook_id.to_string()))
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| async_graphql::Error::new("webhook not found"))?;
        let repo = find_repo(app, webhook.repo_id).await?;
        let perm = repo_permission(app, &repo, claims.sub).await?;
        if perm != Some(Permission::Admin) {
            return Err(async_graphql::Error::new("forbidden"));
        }

        app.db
            .execute("DELETE FROM webhooks WHERE id = ?1", params!(webhook_id.to_string()))
            .await?;
        Ok(true)
    }

    // ---- Label / milestone deletion ----

    /// Deletes a label from a repository (also removing it from any issues
    /// it was attached to, via the `issue_labels` join rows). Requires at
    /// least Write permission on the repository.
    async fn delete_label(&self, ctx: &Context<'_>, label_id: Uuid) -> async_graphql::Result<bool> {
        let app = ctx.data::<AppContext>()?;
        let req = ctx.data::<RequestContext>()?;
        let claims = require_user(req)?;

        let label = app
            .db
            .query_as::<entity::label::Model, _>("SELECT * FROM labels WHERE id = ?1", params!(label_id.to_string()))
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| async_graphql::Error::new("label not found"))?;
        let repo = find_repo(app, label.repo_id).await?;
        let perm = repo_permission(app, &repo, claims.sub).await?;
        if perm < Some(Permission::Write) {
            return Err(async_graphql::Error::new("forbidden"));
        }

        app.db
            .execute(
                "DELETE FROM issue_labels WHERE label_id = ?1",
                params!(label_id.to_string()),
            )
            .await?;
        app.db
            .execute("DELETE FROM labels WHERE id = ?1", params!(label_id.to_string()))
            .await?;
        Ok(true)
    }

    /// Deletes a milestone from a repository, unassigning it from any
    /// issues that referenced it. Requires at least Write permission on
    /// the repository.
    async fn delete_milestone(&self, ctx: &Context<'_>, milestone_id: Uuid) -> async_graphql::Result<bool> {
        let app = ctx.data::<AppContext>()?;
        let req = ctx.data::<RequestContext>()?;
        let claims = require_user(req)?;

        let milestone = app
            .db
            .query_as::<entity::milestone::Model, _>(
                "SELECT * FROM milestones WHERE id = ?1",
                params!(milestone_id.to_string()),
            )
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| async_graphql::Error::new("milestone not found"))?;
        let repo = find_repo(app, milestone.repo_id).await?;
        let perm = repo_permission(app, &repo, claims.sub).await?;
        if perm < Some(Permission::Write) {
            return Err(async_graphql::Error::new("forbidden"));
        }

        app.db
            .execute(
                "UPDATE issues SET milestone_id = NULL WHERE milestone_id = ?1",
                params!(milestone_id.to_string()),
            )
            .await?;
        app.db
            .execute("DELETE FROM milestones WHERE id = ?1", params!(milestone_id.to_string()))
            .await?;
        Ok(true)
    }

    // ---- Dev workspaces: admin/exec ----

    /// Executes a one-off command inside a running dev workspace container
    /// and returns its combined stdout/stderr. Restricted to the
    /// workspace's owner (or a site admin).
    async fn exec_in_dev_workspace(
        &self,
        ctx: &Context<'_>,
        workspace_id: Uuid,
        command: Vec<String>,
    ) -> async_graphql::Result<String> {
        let app = ctx.data::<AppContext>()?;
        let req = ctx.data::<RequestContext>()?;
        let claims = require_user(req)?;

        let workspace = app
            .db
            .query_as::<entity::dev_workspace::Model, _>(
                "SELECT * FROM dev_workspaces WHERE id = ?1",
                params!(workspace_id.to_string()),
            )
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| async_graphql::Error::new("workspace not found"))?;
        if workspace.owner_id != claims.sub && !claims.is_admin {
            return Err(async_graphql::Error::new("forbidden"));
        }
        let container_id = workspace
            .container_id
            .clone()
            .ok_or_else(|| async_graphql::Error::new("workspace has no container"))?;

        if workspace.runner_id.is_some() {
            let payload = serde_json::json!({
                "workspace_id": workspace.id,
                "container_id": container_id,
                "cmd": command,
            });
            let job_id = enqueue_dev_workspace_job(app, "exec", &payload).await?;
            let job = wait_for_runner_job(app, job_id, std::time::Duration::from_secs(20)).await?;
            if job.status != entity::runner_job::status::SUCCESS {
                return Err(async_graphql::Error::new("exec failed on the runner-hosted workspace"));
            }
            let output = job
                .result
                .as_deref()
                .and_then(|r| serde_json::from_str::<serde_json::Value>(r).ok())
                .and_then(|v| v.get("output").and_then(|o| o.as_str()).map(str::to_string))
                .unwrap_or_default();
            return Ok(output);
        }

        let output = app
            .workspace_manager
            .exec_command(&container_id, command)
            .await
            .map_err(|e| async_graphql::Error::new(e.to_string()))?;
        Ok(output)
    }
}

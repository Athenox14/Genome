use async_graphql::{Context, Object};
use auth::{Claims, ClaimsInput, Permission};
use chrono::Utc;
use regex::Regex;
use sea_orm::{
    sea_query::Expr, ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, Set,
};
use std::sync::OnceLock;
use uuid::Uuid;

use crate::context::{AppContext, RequestContext};
use crate::types::{
    resolve_owner_login, AccessTokenCreated, AccessTokenObject, AuthPayload,
    BranchProtectionRuleObject, DevWorkspaceObject, IssueCommentObject, IssueObject, LabelObject,
    MilestoneObject, NotificationObject, OrganizationObject,
    PrReviewCommentObject, PrReviewObject, ProjectCardObject, ProjectColumnObject, ProjectObject,
    PullRequestObject, RepositoryObject, TwoFactorSetup, UserObject, WebhookObject,
    WorkflowRunObject,
};

const TOTP_ISSUER: &str = "Genome";

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
    let notification = entity::notification::ActiveModel {
        id: Set(Uuid::new_v4()),
        user_id: Set(user_id),
        kind: Set(kind.to_string()),
        repo_id: Set(repo_id),
        subject_id: Set(subject_id),
        message: Set(message),
        read_at: Set(None),
        created_at: Set(Utc::now()),
    };
    if let Err(e) = notification.insert(&app.db).await {
        tracing::warn!("failed to insert notification: {e}");
    }
}

/// Best-effort activity feed insert: logs and swallows any error so that a
/// failure to record activity never fails the mutation that triggered it.
async fn record_activity(
    app: &AppContext,
    repo_id: Option<Uuid>,
    actor_id: Uuid,
    kind: &str,
    summary: String,
) {
    let event = entity::activity_event::ActiveModel {
        id: Set(Uuid::new_v4()),
        repo_id: Set(repo_id),
        actor_id: Set(actor_id),
        kind: Set(kind.to_string()),
        summary: Set(summary),
        created_at: Set(Utc::now()),
    };
    if let Err(e) = event.insert(&app.db).await {
        tracing::warn!("failed to insert activity event: {e}");
    }
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

/// Errors unless the given user is an owner or admin of the organization
/// (used to gate org membership management mutations).
async fn require_org_admin(app: &AppContext, org_id: Uuid, user_id: Uuid) -> async_graphql::Result<()> {
    let is_admin = entity::prelude::OrgMember::find()
        .filter(entity::org_member::Column::OrgId.eq(org_id))
        .filter(entity::org_member::Column::UserId.eq(user_id))
        .one(&app.db)
        .await?
        .map(|m| m.role == entity::org_member::role::OWNER || m.role == entity::org_member::role::ADMIN)
        .unwrap_or(false);
    if !is_admin {
        return Err(async_graphql::Error::new("forbidden"));
    }
    Ok(())
}

async fn find_repo(app: &AppContext, repo_id: Uuid) -> async_graphql::Result<entity::repository::Model> {
    entity::prelude::Repository::find_by_id(repo_id)
        .one(&app.db)
        .await?
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
    let rows = entity::prelude::RepoSecret::find()
        .filter(entity::repo_secret::Column::RepoId.eq(repo_id))
        .all(&app.db)
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
            totp_secret: Set(None),
            totp_enabled: Set(false),
            deactivated_at: Set(None),
        };

        let user = user.insert(&app.db).await?;
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

        let key = entity::ssh_key::ActiveModel {
            id: Set(Uuid::new_v4()),
            user_id: Set(claims.sub),
            title: Set(title),
            public_key: Set(public_key),
            fingerprint: Set(fingerprint),
            created_at: Set(Utc::now()),
        };
        let key = key.insert(&app.db).await?;
        Ok(crate::types::SshKeyObject::from(key))
    }

    /// Remove one of the current user's SSH keys.
    async fn remove_ssh_key(&self, ctx: &Context<'_>, key_id: Uuid) -> async_graphql::Result<bool> {
        let req = ctx.data::<RequestContext>()?;
        let claims = require_user(req)?;
        let app = ctx.data::<AppContext>()?;

        let res = entity::prelude::SshKey::delete_many()
            .filter(entity::ssh_key::Column::Id.eq(key_id))
            .filter(entity::ssh_key::Column::UserId.eq(claims.sub))
            .exec(&app.db)
            .await?;
        Ok(res.rows_affected > 0)
    }

    async fn login(
        &self,
        ctx: &Context<'_>,
        username: String,
        password: String,
        totp_code: Option<String>,
    ) -> async_graphql::Result<AuthPayload> {
        let app = ctx.data::<AppContext>()?;
        let user = entity::prelude::User::find()
            .filter(entity::user::Column::Username.eq(username))
            .one(&app.db)
            .await?
            .ok_or_else(|| async_graphql::Error::new("invalid username or password"))?;

        if user.deactivated_at.is_some() {
            return Err(async_graphql::Error::new("account deactivated"));
        }

        let valid = auth::verify_password(&password, &user.password_hash)
            .map_err(|e| async_graphql::Error::new(e.to_string()))?;
        if !valid {
            return Err(async_graphql::Error::new("invalid username or password"));
        }

        if user.totp_enabled {
            let secret = user
                .totp_secret
                .as_deref()
                .ok_or_else(|| async_graphql::Error::new("totp_required"))?;
            match totp_code.as_deref() {
                None => return Err(async_graphql::Error::new("totp_required")),
                Some(code) if !auth::verify_totp(secret, code) => {
                    return Err(async_graphql::Error::new("totp_invalid"));
                }
                _ => {}
            }
        }

        let token = auth::create_jwt(ClaimsInput::from(&user), &app.jwt_secret, 24 * 7)
            .map_err(|e| async_graphql::Error::new(e.to_string()))?;

        Ok(AuthPayload {
            token,
            user: UserObject::from(user),
        })
    }

    /// Begins 2FA setup: generates a new TOTP secret, stores it on the
    /// user's row (with `totp_enabled` still false until confirmed), and
    /// returns the provisioning URI + raw secret for the frontend to render
    /// as a QR code / manual-entry string.
    async fn enable_two_factor(&self, ctx: &Context<'_>) -> async_graphql::Result<TwoFactorSetup> {
        let app = ctx.data::<AppContext>()?;
        let req = ctx.data::<RequestContext>()?;
        let claims = require_user(req)?;

        let user = entity::prelude::User::find_by_id(claims.sub)
            .one(&app.db)
            .await?
            .ok_or_else(|| async_graphql::Error::new("user not found"))?;

        let secret = auth::generate_totp_secret();
        let provisioning_uri = auth::totp_provisioning_uri(&secret, &user.username, TOTP_ISSUER);

        let mut active: entity::user::ActiveModel = user.into();
        active.totp_secret = Set(Some(secret.clone()));
        active.totp_enabled = Set(false);
        active.update(&app.db).await?;

        Ok(TwoFactorSetup {
            secret,
            provisioning_uri,
        })
    }

    /// Confirms 2FA setup by verifying a code against the pending secret
    /// stored by `enableTwoFactor`, then flips `totp_enabled` to true.
    async fn confirm_two_factor(&self, ctx: &Context<'_>, code: String) -> async_graphql::Result<bool> {
        let app = ctx.data::<AppContext>()?;
        let req = ctx.data::<RequestContext>()?;
        let claims = require_user(req)?;

        let user = entity::prelude::User::find_by_id(claims.sub)
            .one(&app.db)
            .await?
            .ok_or_else(|| async_graphql::Error::new("user not found"))?;

        let secret = user
            .totp_secret
            .clone()
            .ok_or_else(|| async_graphql::Error::new("no pending two-factor setup"))?;

        if !auth::verify_totp(&secret, &code) {
            return Err(async_graphql::Error::new("totp_invalid"));
        }

        let mut active: entity::user::ActiveModel = user.into();
        active.totp_enabled = Set(true);
        active.update(&app.db).await?;

        Ok(true)
    }

    /// Disables 2FA for the current user, clearing the stored secret.
    async fn disable_two_factor(&self, ctx: &Context<'_>) -> async_graphql::Result<bool> {
        let app = ctx.data::<AppContext>()?;
        let req = ctx.data::<RequestContext>()?;
        let claims = require_user(req)?;

        let user = entity::prelude::User::find_by_id(claims.sub)
            .one(&app.db)
            .await?
            .ok_or_else(|| async_graphql::Error::new("user not found"))?;

        let mut active: entity::user::ActiveModel = user.into();
        active.totp_enabled = Set(false);
        active.totp_secret = Set(None);
        active.update(&app.db).await?;

        Ok(true)
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

        let user = entity::prelude::User::find_by_id(user_id)
            .one(&app.db)
            .await?
            .ok_or_else(|| async_graphql::Error::new("user not found"))?;

        let mut active: entity::user::ActiveModel = user.into();
        active.is_admin = Set(is_admin);
        let user = active.update(&app.db).await?;

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

        let user = entity::prelude::User::find_by_id(user_id)
            .one(&app.db)
            .await?
            .ok_or_else(|| async_graphql::Error::new("user not found"))?;

        let mut active: entity::user::ActiveModel = user.into();
        active.deactivated_at = Set(Some(Utc::now()));
        let user = active.update(&app.db).await?;

        Ok(UserObject::from(user))
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

        if let Err(e) = app.repo_manager.init_wiki(&owner.username, &name) {
            tracing::warn!("failed to initialize wiki repo for {}/{name}: {e}", owner.username);
        }

        let repo = entity::repository::ActiveModel {
            id: Set(Uuid::new_v4()),
            owner_type: Set("user".to_string()),
            owner_id: Set(owner.id),
            name: Set(name),
            description: Set(description),
            is_private: Set(is_private),
            default_branch: Set("main".to_string()),
            forked_from_id: Set(None),
            created_at: Set(Utc::now()),
        };
        let repo = repo.insert(&app.db).await?;

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

        entity::prelude::Repository::delete_by_id(repo.id)
            .exec(&app.db)
            .await?;

        Ok(true)
    }

    /// Updates a repository's mutable metadata (description, visibility,
    /// default branch). Requires Admin permission on the repository. Does
    /// not rename the repository on disk; renaming is not currently
    /// supported.
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

        let repo = find_repo(app, repo_id).await?;
        let perm = repo_permission(app, &repo, claims.sub).await?;
        if perm != Some(Permission::Admin) {
            return Err(async_graphql::Error::new("forbidden"));
        }

        let mut active: entity::repository::ActiveModel = repo.into();
        if let Some(description) = description {
            active.description = Set(Some(description));
        }
        if let Some(is_private) = is_private {
            active.is_private = Set(is_private);
        }
        if let Some(default_branch) = default_branch {
            active.default_branch = Set(default_branch);
        }
        let repo = active.update(&app.db).await?;

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

        let dest_owner = entity::prelude::User::find_by_id(claims.sub)
            .one(&app.db)
            .await?
            .ok_or_else(|| async_graphql::Error::new("user not found"))?;

        let existing = entity::prelude::Repository::find()
            .filter(entity::repository::Column::OwnerType.eq("user"))
            .filter(entity::repository::Column::OwnerId.eq(dest_owner.id))
            .filter(entity::repository::Column::Name.eq(source_repo.name.clone()))
            .one(&app.db)
            .await?;
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

        let repo = entity::repository::ActiveModel {
            id: Set(Uuid::new_v4()),
            owner_type: Set("user".to_string()),
            owner_id: Set(dest_owner.id),
            name: Set(source_repo.name.clone()),
            description: Set(source_repo.description.clone()),
            is_private: Set(source_repo.is_private),
            default_branch: Set(source_repo.default_branch.clone()),
            created_at: Set(Utc::now()),
            forked_from_id: Set(Some(source_repo.id)),
        };
        let repo = repo.insert(&app.db).await?;

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
            milestone_id: Set(None),
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

        let issue = entity::prelude::Issue::find_by_id(issue_id)
            .one(&app.db)
            .await?
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
        let mut active: entity::issue::ActiveModel = issue.into();
        if let Some(title) = title {
            active.title = Set(title);
        }
        if let Some(body) = body {
            active.body = Set(Some(body));
        }
        if let Some(state) = state {
            let now_closed = state == entity::issue::state::CLOSED;
            active.closed_at = Set(if now_closed { Some(Utc::now()) } else { None });
            active.state = Set(state);
        }
        let issue = active.update(&app.db).await?;

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

        let pr = entity::prelude::PullRequest::find_by_id(pr_id)
            .one(&app.db)
            .await?
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
        let rules = entity::prelude::BranchProtectionRule::find()
            .filter(entity::branch_protection_rule::Column::RepoId.eq(repo.id))
            .all(&app.db)
            .await?;
        let matching_rule = rules.into_iter().find(|r| {
            entity::branch_protection_rule::branch_matches_pattern(&r.branch_pattern, &pr.target_branch)
        });
        if let Some(rule) = matching_rule {
            if rule.require_reviews_count > 0 {
                let approved_count = entity::prelude::PrReview::find()
                    .filter(entity::pr_review::Column::PrId.eq(pr.id))
                    .filter(entity::pr_review::Column::State.eq(entity::pr_review::state::APPROVED))
                    .all(&app.db)
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

        let pr = entity::prelude::PullRequest::find_by_id(pr_id)
            .one(&app.db)
            .await?
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

        let mut active: entity::pull_request::ActiveModel = pr.into();
        active.state = Set(entity::pull_request::state::CLOSED.to_string());
        let pr = active.update(&app.db).await?;

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

        let rule = entity::branch_protection_rule::ActiveModel {
            id: Set(Uuid::new_v4()),
            repo_id: Set(repo_id),
            branch_pattern: Set(branch_pattern),
            require_reviews_count: Set(require_reviews_count),
            require_status_checks: Set(false),
            block_force_push: Set(block_force_push),
            created_at: Set(Utc::now()),
        };
        let rule = rule.insert(&app.db).await?;

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

        let pr = entity::prelude::PullRequest::find_by_id(pr_id)
            .one(&app.db)
            .await?
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

        let review = entity::pr_review::ActiveModel {
            id: Set(Uuid::new_v4()),
            pr_id: Set(pr_id),
            reviewer_id: Set(claims.sub),
            state: Set(state),
            body: Set(body),
            created_at: Set(Utc::now()),
        };
        let review = review.insert(&app.db).await?;

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

        let review = entity::prelude::PrReview::find_by_id(review_id)
            .one(&app.db)
            .await?
            .ok_or_else(|| async_graphql::Error::new("review not found"))?;
        let pr = entity::prelude::PullRequest::find_by_id(review.pr_id)
            .one(&app.db)
            .await?
            .ok_or_else(|| async_graphql::Error::new("pull request not found"))?;
        let repo = find_repo(app, pr.repo_id).await?;
        let perm = repo_permission(app, &repo, claims.sub).await?;
        if perm.is_none() {
            return Err(async_graphql::Error::new("forbidden"));
        }

        let comment = entity::pr_review_comment::ActiveModel {
            id: Set(Uuid::new_v4()),
            review_id: Set(review_id),
            file_path: Set(file_path),
            line_number: Set(line_number),
            body: Set(body),
            created_at: Set(Utc::now()),
        };
        let comment = comment.insert(&app.db).await?;

        Ok(PrReviewCommentObject::from(comment))
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

        let user = entity::prelude::User::find()
            .filter(entity::user::Column::Username.eq(username))
            .one(&app.db)
            .await?
            .ok_or_else(|| async_graphql::Error::new("user not found"))?;

        let member = entity::org_member::ActiveModel {
            org_id: Set(org_id),
            user_id: Set(user.id),
            role: Set(role),
        };
        member.insert(&app.db).await?;
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

        let member = entity::prelude::OrgMember::find()
            .filter(entity::org_member::Column::OrgId.eq(org_id))
            .filter(entity::org_member::Column::UserId.eq(user_id))
            .one(&app.db)
            .await?
            .ok_or_else(|| async_graphql::Error::new("member not found"))?;

        let mut active: entity::org_member::ActiveModel = member.into();
        active.role = Set(role);
        active.update(&app.db).await?;
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

        let res = entity::prelude::OrgMember::delete_many()
            .filter(entity::org_member::Column::OrgId.eq(org_id))
            .filter(entity::org_member::Column::UserId.eq(user_id))
            .exec(&app.db)
            .await?;
        Ok(res.rows_affected > 0)
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

        let is_owner = entity::prelude::OrgMember::find()
            .filter(entity::org_member::Column::OrgId.eq(org_id))
            .filter(entity::org_member::Column::UserId.eq(claims.sub))
            .one(&app.db)
            .await?
            .map(|m| m.role == entity::org_member::role::OWNER)
            .unwrap_or(false);
        if !is_owner {
            return Err(async_graphql::Error::new("forbidden"));
        }

        entity::prelude::OrgMember::delete_many()
            .filter(entity::org_member::Column::OrgId.eq(org_id))
            .exec(&app.db)
            .await?;
        entity::prelude::Organization::delete_by_id(org_id)
            .exec(&app.db)
            .await?;
        Ok(true)
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

        let res = entity::prelude::RepoCollaborator::delete_many()
            .filter(entity::repo_collaborator::Column::RepoId.eq(repo_id))
            .filter(entity::repo_collaborator::Column::UserId.eq(user_id))
            .exec(&app.db)
            .await?;
        Ok(res.rows_affected > 0)
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
        let secrets = load_repo_secrets(app, repo_id).await.unwrap_or_default();
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
                            let row = entity::workflow_artifact::ActiveModel {
                                id: Set(Uuid::new_v4()),
                                run_id: Set(run_id),
                                job_id: Set(None),
                                name: Set(artifact.name.clone()),
                                file_path: Set(artifact.file_path.clone()),
                                size_bytes: Set(artifact.size_bytes),
                                created_at: Set(Utc::now()),
                            };
                            if let Err(e) = row.insert(&db).await {
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

    async fn create_dev_workspace(
        &self,
        ctx: &Context<'_>,
        name: String,
        image: Option<String>,
        repo_id: Option<Uuid>,
        auto_stop_minutes: Option<i32>,
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
            auto_stop_minutes: Set(auto_stop_minutes),
            last_activity_at: Set(Some(Utc::now())),
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

    // ---- Labels ----

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

        let label = entity::label::ActiveModel {
            id: Set(Uuid::new_v4()),
            repo_id: Set(repo_id),
            name: Set(name),
            color: Set(color),
        };
        let label = label.insert(&app.db).await?;
        Ok(LabelObject::from(label))
    }

    async fn add_label_to_issue(
        &self,
        ctx: &Context<'_>,
        issue_id: Uuid,
        label_id: Uuid,
    ) -> async_graphql::Result<IssueObject> {
        let app = ctx.data::<AppContext>()?;
        let req = ctx.data::<RequestContext>()?;
        let claims = require_user(req)?;

        let issue = entity::prelude::Issue::find_by_id(issue_id)
            .one(&app.db)
            .await?
            .ok_or_else(|| async_graphql::Error::new("issue not found"))?;
        let repo = find_repo(app, issue.repo_id).await?;
        let perm = repo_permission(app, &repo, claims.sub).await?;
        if perm < Some(Permission::Write) {
            return Err(async_graphql::Error::new("forbidden"));
        }

        let label = entity::prelude::Label::find_by_id(label_id)
            .one(&app.db)
            .await?
            .ok_or_else(|| async_graphql::Error::new("label not found"))?;
        if label.repo_id != repo.id {
            return Err(async_graphql::Error::new("label does not belong to this repository"));
        }

        let existing = entity::prelude::IssueLabel::find()
            .filter(entity::issue_label::Column::IssueId.eq(issue_id))
            .filter(entity::issue_label::Column::LabelId.eq(label_id))
            .one(&app.db)
            .await?;
        if existing.is_none() {
            let link = entity::issue_label::ActiveModel {
                issue_id: Set(issue_id),
                label_id: Set(label_id),
            };
            link.insert(&app.db).await?;
        }

        Ok(IssueObject::from(issue))
    }

    async fn remove_label_from_issue(
        &self,
        ctx: &Context<'_>,
        issue_id: Uuid,
        label_id: Uuid,
    ) -> async_graphql::Result<IssueObject> {
        let app = ctx.data::<AppContext>()?;
        let req = ctx.data::<RequestContext>()?;
        let claims = require_user(req)?;

        let issue = entity::prelude::Issue::find_by_id(issue_id)
            .one(&app.db)
            .await?
            .ok_or_else(|| async_graphql::Error::new("issue not found"))?;
        let repo = find_repo(app, issue.repo_id).await?;
        let perm = repo_permission(app, &repo, claims.sub).await?;
        if perm < Some(Permission::Write) {
            return Err(async_graphql::Error::new("forbidden"));
        }

        entity::prelude::IssueLabel::delete_many()
            .filter(entity::issue_label::Column::IssueId.eq(issue_id))
            .filter(entity::issue_label::Column::LabelId.eq(label_id))
            .exec(&app.db)
            .await?;

        Ok(IssueObject::from(issue))
    }

    // ---- Milestones ----

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

        let milestone = entity::milestone::ActiveModel {
            id: Set(Uuid::new_v4()),
            repo_id: Set(repo_id),
            title: Set(title),
            description: Set(description),
            due_date: Set(due_date),
            state: Set(entity::milestone::state::OPEN.to_string()),
            created_at: Set(Utc::now()),
        };
        let milestone = milestone.insert(&app.db).await?;
        Ok(MilestoneObject::from(milestone))
    }

    async fn set_issue_milestone(
        &self,
        ctx: &Context<'_>,
        issue_id: Uuid,
        milestone_id: Option<Uuid>,
    ) -> async_graphql::Result<IssueObject> {
        let app = ctx.data::<AppContext>()?;
        let req = ctx.data::<RequestContext>()?;
        let claims = require_user(req)?;

        let issue = entity::prelude::Issue::find_by_id(issue_id)
            .one(&app.db)
            .await?
            .ok_or_else(|| async_graphql::Error::new("issue not found"))?;
        let repo = find_repo(app, issue.repo_id).await?;
        let perm = repo_permission(app, &repo, claims.sub).await?;
        if perm < Some(Permission::Write) {
            return Err(async_graphql::Error::new("forbidden"));
        }

        if let Some(mid) = milestone_id {
            let milestone = entity::prelude::Milestone::find_by_id(mid)
                .one(&app.db)
                .await?
                .ok_or_else(|| async_graphql::Error::new("milestone not found"))?;
            if milestone.repo_id != repo.id {
                return Err(async_graphql::Error::new(
                    "milestone does not belong to this repository",
                ));
            }
        }

        let mut active: entity::issue::ActiveModel = issue.into();
        active.milestone_id = Set(milestone_id);
        let issue = active.update(&app.db).await?;

        Ok(IssueObject::from(issue))
    }

    // ---- Projects ----

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

        let project = entity::project::ActiveModel {
            id: Set(Uuid::new_v4()),
            repo_id: Set(repo_id),
            name: Set(name),
            created_at: Set(Utc::now()),
        };
        let project = project.insert(&app.db).await?;
        Ok(ProjectObject::from(project))
    }

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

        let project = entity::prelude::Project::find_by_id(project_id)
            .one(&app.db)
            .await?
            .ok_or_else(|| async_graphql::Error::new("project not found"))?;
        let repo = find_repo(app, project.repo_id).await?;
        let perm = repo_permission(app, &repo, claims.sub).await?;
        if perm < Some(Permission::Write) {
            return Err(async_graphql::Error::new("forbidden"));
        }

        let column = entity::project_column::ActiveModel {
            id: Set(Uuid::new_v4()),
            project_id: Set(project_id),
            name: Set(name),
            position: Set(position),
        };
        let column = column.insert(&app.db).await?;
        Ok(ProjectColumnObject::from(column))
    }

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

        let column = entity::prelude::ProjectColumn::find_by_id(column_id)
            .one(&app.db)
            .await?
            .ok_or_else(|| async_graphql::Error::new("project column not found"))?;
        let project = entity::prelude::Project::find_by_id(column.project_id)
            .one(&app.db)
            .await?
            .ok_or_else(|| async_graphql::Error::new("project not found"))?;
        let repo = find_repo(app, project.repo_id).await?;
        let perm = repo_permission(app, &repo, claims.sub).await?;
        if perm < Some(Permission::Write) {
            return Err(async_graphql::Error::new("forbidden"));
        }

        let issue = entity::prelude::Issue::find_by_id(issue_id)
            .one(&app.db)
            .await?
            .ok_or_else(|| async_graphql::Error::new("issue not found"))?;
        if issue.repo_id != repo.id {
            return Err(async_graphql::Error::new("issue does not belong to this repository"));
        }

        let card = entity::project_card::ActiveModel {
            id: Set(Uuid::new_v4()),
            column_id: Set(column_id),
            issue_id: Set(Some(issue_id)),
            pull_request_id: Set(None),
            position: Set(position),
        };
        let card = card.insert(&app.db).await?;
        Ok(ProjectCardObject::from(card))
    }

    // ---- Notifications ----

    /// Marks a notification as read. Only the notification's owner may do this.
    async fn mark_notification_read(&self, ctx: &Context<'_>, id: Uuid) -> async_graphql::Result<NotificationObject> {
        let app = ctx.data::<AppContext>()?;
        let req = ctx.data::<RequestContext>()?;
        let claims = require_user(req)?;

        let notification = entity::prelude::Notification::find_by_id(id)
            .one(&app.db)
            .await?
            .ok_or_else(|| async_graphql::Error::new("notification not found"))?;
        if notification.user_id != claims.sub {
            return Err(async_graphql::Error::new("forbidden"));
        }

        let mut active: entity::notification::ActiveModel = notification.into();
        active.read_at = Set(Some(Utc::now()));
        let notification = active.update(&app.db).await?;

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

        let existing = entity::prelude::RepoSecret::find()
            .filter(entity::repo_secret::Column::RepoId.eq(repo_id))
            .filter(entity::repo_secret::Column::Name.eq(name.clone()))
            .one(&app.db)
            .await?;

        if let Some(existing) = existing {
            let mut active: entity::repo_secret::ActiveModel = existing.into();
            active.encrypted_value = Set(encrypted_value);
            active.update(&app.db).await?;
        } else {
            let secret = entity::repo_secret::ActiveModel {
                id: Set(Uuid::new_v4()),
                repo_id: Set(repo_id),
                name: Set(name),
                encrypted_value: Set(encrypted_value),
                created_at: Set(Utc::now()),
            };
            secret.insert(&app.db).await?;
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

        let existing = entity::prelude::RepoMirror::find()
            .filter(entity::repo_mirror::Column::RepoId.eq(repo_id))
            .one(&app.db)
            .await?;

        if let Some(existing) = existing {
            let mut active: entity::repo_mirror::ActiveModel = existing.into();
            active.remote_url = Set(remote_url);
            active.sync_interval_minutes = Set(sync_interval_minutes);
            active.update(&app.db).await?;
        } else {
            let mirror = entity::repo_mirror::ActiveModel {
                id: Set(Uuid::new_v4()),
                repo_id: Set(repo_id),
                remote_url: Set(remote_url),
                last_synced_at: Set(None),
                sync_interval_minutes: Set(sync_interval_minutes),
                created_at: Set(Utc::now()),
            };
            mirror.insert(&app.db).await?;
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

        let row = entity::access_token::ActiveModel {
            id: Set(Uuid::new_v4()),
            user_id: Set(claims.sub),
            token_hash: Set(token_hash),
            name: Set(name),
            scopes: Set(serde_json::to_value(&scopes).unwrap_or(serde_json::Value::Array(vec![]))),
            expires_at: Set(expires_at),
        };
        let row = row.insert(&app.db).await?;

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

        let res = entity::prelude::AccessToken::delete_many()
            .filter(entity::access_token::Column::Id.eq(id))
            .filter(entity::access_token::Column::UserId.eq(claims.sub))
            .exec(&app.db)
            .await?;
        Ok(res.rows_affected > 0)
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

        let webhook = entity::webhook::ActiveModel {
            id: Set(Uuid::new_v4()),
            repo_id: Set(repo_id),
            target_url: Set(target_url),
            secret: Set(secret),
            events: Set(serde_json::to_value(&events).unwrap_or(serde_json::Value::Array(vec![]))),
            active: Set(true),
        };
        let webhook = webhook.insert(&app.db).await?;
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

        let webhook = entity::prelude::Webhook::find_by_id(webhook_id)
            .one(&app.db)
            .await?
            .ok_or_else(|| async_graphql::Error::new("webhook not found"))?;
        let repo = find_repo(app, webhook.repo_id).await?;
        let perm = repo_permission(app, &repo, claims.sub).await?;
        if perm != Some(Permission::Admin) {
            return Err(async_graphql::Error::new("forbidden"));
        }

        let mut active_model: entity::webhook::ActiveModel = webhook.into();
        if let Some(target_url) = target_url {
            active_model.target_url = Set(target_url);
        }
        if let Some(events) = events {
            active_model.events =
                Set(serde_json::to_value(&events).unwrap_or(serde_json::Value::Array(vec![])));
        }
        if let Some(active) = active {
            active_model.active = Set(active);
        }
        let webhook = active_model.update(&app.db).await?;
        Ok(WebhookObject::from(webhook))
    }

    /// Deletes a webhook. Requires Admin permission on the repository.
    async fn delete_webhook(&self, ctx: &Context<'_>, webhook_id: Uuid) -> async_graphql::Result<bool> {
        let app = ctx.data::<AppContext>()?;
        let req = ctx.data::<RequestContext>()?;
        let claims = require_user(req)?;

        let webhook = entity::prelude::Webhook::find_by_id(webhook_id)
            .one(&app.db)
            .await?
            .ok_or_else(|| async_graphql::Error::new("webhook not found"))?;
        let repo = find_repo(app, webhook.repo_id).await?;
        let perm = repo_permission(app, &repo, claims.sub).await?;
        if perm != Some(Permission::Admin) {
            return Err(async_graphql::Error::new("forbidden"));
        }

        entity::prelude::Webhook::delete_by_id(webhook_id)
            .exec(&app.db)
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

        let label = entity::prelude::Label::find_by_id(label_id)
            .one(&app.db)
            .await?
            .ok_or_else(|| async_graphql::Error::new("label not found"))?;
        let repo = find_repo(app, label.repo_id).await?;
        let perm = repo_permission(app, &repo, claims.sub).await?;
        if perm < Some(Permission::Write) {
            return Err(async_graphql::Error::new("forbidden"));
        }

        entity::prelude::IssueLabel::delete_many()
            .filter(entity::issue_label::Column::LabelId.eq(label_id))
            .exec(&app.db)
            .await?;
        entity::prelude::Label::delete_by_id(label_id)
            .exec(&app.db)
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

        let milestone = entity::prelude::Milestone::find_by_id(milestone_id)
            .one(&app.db)
            .await?
            .ok_or_else(|| async_graphql::Error::new("milestone not found"))?;
        let repo = find_repo(app, milestone.repo_id).await?;
        let perm = repo_permission(app, &repo, claims.sub).await?;
        if perm < Some(Permission::Write) {
            return Err(async_graphql::Error::new("forbidden"));
        }

        entity::prelude::Issue::update_many()
            .col_expr(entity::issue::Column::MilestoneId, Expr::value(None::<Uuid>))
            .filter(entity::issue::Column::MilestoneId.eq(milestone_id))
            .exec(&app.db)
            .await?;
        entity::prelude::Milestone::delete_by_id(milestone_id)
            .exec(&app.db)
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

        let workspace = entity::prelude::DevWorkspace::find_by_id(workspace_id)
            .one(&app.db)
            .await?
            .ok_or_else(|| async_graphql::Error::new("workspace not found"))?;
        if workspace.owner_id != claims.sub && !claims.is_admin {
            return Err(async_graphql::Error::new("forbidden"));
        }
        let container_id = workspace
            .container_id
            .clone()
            .ok_or_else(|| async_graphql::Error::new("workspace has no container"))?;

        let output = app
            .workspace_manager
            .exec_command(&container_id, command)
            .await
            .map_err(|e| async_graphql::Error::new(e.to_string()))?;
        Ok(output)
    }
}

//! Duplicated (small, on purpose) permission-resolution helper.
//!
//! This mirrors `graphql-api::mutation::repo_permission` / the smart-HTTP
//! authorization pattern, but lives in this crate since `graphql-api` is not
//! a dependency here and the logic is small enough not to warrant extracting
//! a shared crate.

use hiqlite::params;
use uuid::Uuid;

use auth::Permission;

use crate::error::SshServerError;

/// Resolve `owner_login` (a username or organization name) plus `repo_name`
/// to the repository row, if any exists.
pub async fn find_repo(
    db: &hiqlite::Client,
    owner_login: &str,
    repo_name: &str,
) -> Result<Option<entity::repository::Model>, SshServerError> {
    let user = db
        .query_as::<entity::user::Model, _>(
            "SELECT * FROM users WHERE username = ?1",
            params!(owner_login.to_string()),
        )
        .await?
        .into_iter()
        .next();

    let (owner_type, owner_id) = if let Some(u) = user {
        ("user".to_string(), u.id)
    } else if let Some(org) = db
        .query_as::<entity::organization::Model, _>(
            "SELECT * FROM organizations WHERE name = ?1",
            params!(owner_login.to_string()),
        )
        .await?
        .into_iter()
        .next()
    {
        ("organization".to_string(), org.id)
    } else {
        return Ok(None);
    };

    let repo = db
        .query_as::<entity::repository::Model, _>(
            "SELECT * FROM repositories WHERE owner_type = ?1 AND owner_id = ?2 AND name = ?3",
            params!(owner_type, owner_id.to_string(), repo_name.to_string()),
        )
        .await?
        .into_iter()
        .next();

    Ok(repo)
}

/// Determine the effective permission `user_id` has on `repo`.
pub async fn repo_permission(
    db: &hiqlite::Client,
    repo: &entity::repository::Model,
    user_id: Uuid,
) -> Result<Option<Permission>, SshServerError> {
    let is_owner = repo.owner_type == "user" && repo.owner_id == user_id;

    let is_admin_org_role = if repo.owner_type == "organization" {
        db.query_as::<entity::org_member::Model, _>(
            "SELECT * FROM org_members WHERE org_id = ?1 AND user_id = ?2",
            params!(repo.owner_id.to_string(), user_id.to_string()),
        )
        .await?
        .into_iter()
        .next()
        .map(|m| {
            m.role == entity::org_member::role::OWNER
                || m.role == entity::org_member::role::ADMIN
        })
        .unwrap_or(false)
    } else {
        false
    };

    let collaborator_perm = db
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

//! Duplicated (small, on purpose) permission-resolution helper.
//!
//! This mirrors `graphql-api::mutation::repo_permission` / the smart-HTTP
//! authorization pattern, but lives in this crate since `graphql-api` is not
//! a dependency here and the logic is small enough not to warrant extracting
//! a shared crate.

use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter};
use uuid::Uuid;

use auth::Permission;

/// Resolve `owner_login` (a username or organization name) plus `repo_name`
/// to the repository row, if any exists.
pub async fn find_repo(
    db: &DatabaseConnection,
    owner_login: &str,
    repo_name: &str,
) -> Result<Option<entity::repository::Model>, sea_orm::DbErr> {
    let user = entity::prelude::User::find()
        .filter(entity::user::Column::Username.eq(owner_login))
        .one(db)
        .await?;

    let (owner_type, owner_id) = if let Some(u) = user {
        ("user".to_string(), u.id)
    } else if let Some(org) = entity::prelude::Organization::find()
        .filter(entity::organization::Column::Name.eq(owner_login))
        .one(db)
        .await?
    {
        ("organization".to_string(), org.id)
    } else {
        return Ok(None);
    };

    entity::prelude::Repository::find()
        .filter(entity::repository::Column::OwnerType.eq(owner_type))
        .filter(entity::repository::Column::OwnerId.eq(owner_id))
        .filter(entity::repository::Column::Name.eq(repo_name))
        .one(db)
        .await
}

/// Determine the effective permission `user_id` has on `repo`.
pub async fn repo_permission(
    db: &DatabaseConnection,
    repo: &entity::repository::Model,
    user_id: Uuid,
) -> Result<Option<Permission>, sea_orm::DbErr> {
    let is_owner = repo.owner_type == "user" && repo.owner_id == user_id;

    let is_admin_org_role = if repo.owner_type == "organization" {
        entity::prelude::OrgMember::find()
            .filter(entity::org_member::Column::OrgId.eq(repo.owner_id))
            .filter(entity::org_member::Column::UserId.eq(user_id))
            .one(db)
            .await?
            .map(|m| {
                m.role == entity::org_member::role::OWNER
                    || m.role == entity::org_member::role::ADMIN
            })
            .unwrap_or(false)
    } else {
        false
    };

    let collaborator_perm = entity::prelude::RepoCollaborator::find()
        .filter(entity::repo_collaborator::Column::RepoId.eq(repo.id))
        .filter(entity::repo_collaborator::Column::UserId.eq(user_id))
        .one(db)
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

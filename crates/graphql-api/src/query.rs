use async_graphql::{Context, Object};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use uuid::Uuid;

use crate::context::{AppContext, RequestContext};
use crate::types::{DevWorkspaceObject, OrganizationObject, RepositoryObject, UserObject};

pub struct QueryRoot;

#[Object]
impl QueryRoot {
    /// The currently authenticated user, if any.
    async fn me(&self, ctx: &Context<'_>) -> async_graphql::Result<Option<UserObject>> {
        let app = ctx.data::<AppContext>()?;
        let req = ctx.data::<RequestContext>()?;
        let Some(claims) = &req.user else {
            return Ok(None);
        };
        let user = entity::prelude::User::find_by_id(claims.sub)
            .one(&app.db)
            .await?;
        Ok(user.map(UserObject::from))
    }

    /// Look up a user by username.
    async fn user(&self, ctx: &Context<'_>, username: String) -> async_graphql::Result<Option<UserObject>> {
        let app = ctx.data::<AppContext>()?;
        let user = entity::prelude::User::find()
            .filter(entity::user::Column::Username.eq(username))
            .one(&app.db)
            .await?;
        Ok(user.map(UserObject::from))
    }

    /// Repositories owned by, or shared with, the current user.
    async fn my_repositories(&self, ctx: &Context<'_>) -> async_graphql::Result<Vec<RepositoryObject>> {
        let app = ctx.data::<AppContext>()?;
        let req = ctx.data::<RequestContext>()?;
        let Some(claims) = &req.user else {
            return Err(async_graphql::Error::new("unauthenticated"));
        };

        let owned = entity::prelude::Repository::find()
            .filter(entity::repository::Column::OwnerType.eq("user"))
            .filter(entity::repository::Column::OwnerId.eq(claims.sub))
            .all(&app.db)
            .await?;

        let collab_repo_ids: Vec<Uuid> = entity::prelude::RepoCollaborator::find()
            .filter(entity::repo_collaborator::Column::UserId.eq(claims.sub))
            .all(&app.db)
            .await?
            .into_iter()
            .map(|c| c.repo_id)
            .collect();

        let mut repos = owned;
        if !collab_repo_ids.is_empty() {
            let collab_repos = entity::prelude::Repository::find()
                .filter(entity::repository::Column::Id.is_in(collab_repo_ids))
                .all(&app.db)
                .await?;
            for r in collab_repos {
                if !repos.iter().any(|existing| existing.id == r.id) {
                    repos.push(r);
                }
            }
        }

        let mut out = Vec::with_capacity(repos.len());
        for r in repos {
            out.push(RepositoryObject::from_model(&app.db, r).await);
        }
        Ok(out)
    }

    /// Look up a repository by owner login and name.
    async fn repository(
        &self,
        ctx: &Context<'_>,
        owner: String,
        name: String,
    ) -> async_graphql::Result<Option<RepositoryObject>> {
        let app = ctx.data::<AppContext>()?;

        // Resolve owner login -> (owner_type, owner_id).
        let user = entity::prelude::User::find()
            .filter(entity::user::Column::Username.eq(owner.clone()))
            .one(&app.db)
            .await?;

        let (owner_type, owner_id) = if let Some(u) = user {
            ("user".to_string(), u.id)
        } else if let Some(org) = entity::prelude::Organization::find()
            .filter(entity::organization::Column::Name.eq(owner.clone()))
            .one(&app.db)
            .await?
        {
            ("organization".to_string(), org.id)
        } else {
            return Ok(None);
        };

        let repo = entity::prelude::Repository::find()
            .filter(entity::repository::Column::OwnerType.eq(owner_type))
            .filter(entity::repository::Column::OwnerId.eq(owner_id))
            .filter(entity::repository::Column::Name.eq(name))
            .one(&app.db)
            .await?;

        match repo {
            Some(r) => Ok(Some(RepositoryObject::from_model(&app.db, r).await)),
            None => Ok(None),
        }
    }

    /// Organizations the current user belongs to (or all, if unauthenticated is not allowed -
    /// here we simply return all organizations, which are not sensitive).
    async fn organizations(&self, ctx: &Context<'_>) -> async_graphql::Result<Vec<OrganizationObject>> {
        let app = ctx.data::<AppContext>()?;
        let orgs = entity::prelude::Organization::find().all(&app.db).await?;
        Ok(orgs.into_iter().map(OrganizationObject::from).collect())
    }

    /// Look up an organization by name.
    async fn organization(
        &self,
        ctx: &Context<'_>,
        name: String,
    ) -> async_graphql::Result<Option<OrganizationObject>> {
        let app = ctx.data::<AppContext>()?;
        let org = entity::prelude::Organization::find()
            .filter(entity::organization::Column::Name.eq(name))
            .one(&app.db)
            .await?;
        Ok(org.map(OrganizationObject::from))
    }

    /// Dev workspaces belonging to the current user.
    async fn dev_workspaces(&self, ctx: &Context<'_>) -> async_graphql::Result<Vec<DevWorkspaceObject>> {
        let app = ctx.data::<AppContext>()?;
        let req = ctx.data::<RequestContext>()?;
        let Some(claims) = &req.user else {
            return Err(async_graphql::Error::new("unauthenticated"));
        };
        let workspaces = entity::prelude::DevWorkspace::find()
            .filter(entity::dev_workspace::Column::OwnerId.eq(claims.sub))
            .all(&app.db)
            .await?;
        Ok(workspaces.into_iter().map(DevWorkspaceObject::from).collect())
    }
}

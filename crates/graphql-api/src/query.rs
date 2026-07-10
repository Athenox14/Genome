use async_graphql::{Context, Object};
use sea_orm::{
    sea_query::Expr, ColumnTrait, Condition, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder,
    QuerySelect,
};
use uuid::Uuid;

use crate::context::{AppContext, RequestContext};
use crate::types::{
    AccessTokenObject, ActivityEventObject, DevWorkspaceObject, IssueObject, NotificationObject,
    OrganizationObject, PackageObject, RepositoryObject, SearchResults, SshKeyObject, UserObject,
};

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

    /// The current user's registered SSH public keys (for git-over-SSH auth).
    async fn my_ssh_keys(&self, ctx: &Context<'_>) -> async_graphql::Result<Vec<SshKeyObject>> {
        let app = ctx.data::<AppContext>()?;
        let req = ctx.data::<RequestContext>()?;
        let Some(claims) = &req.user else {
            return Err(async_graphql::Error::new("unauthenticated"));
        };
        let keys = entity::prelude::SshKey::find()
            .filter(entity::ssh_key::Column::UserId.eq(claims.sub))
            .all(&app.db)
            .await?;
        Ok(keys.into_iter().map(SshKeyObject::from).collect())
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

    /// Every dev workspace on the instance, regardless of owner. Restricted
    /// to site admins.
    async fn admin_all_dev_workspaces(&self, ctx: &Context<'_>) -> async_graphql::Result<Vec<DevWorkspaceObject>> {
        let app = ctx.data::<AppContext>()?;
        let req = ctx.data::<RequestContext>()?;
        let Some(claims) = &req.user else {
            return Err(async_graphql::Error::new("unauthenticated"));
        };
        if !claims.is_admin {
            return Err(async_graphql::Error::new("forbidden"));
        }
        let workspaces = entity::prelude::DevWorkspace::find().all(&app.db).await?;
        Ok(workspaces.into_iter().map(DevWorkspaceObject::from).collect())
    }

    /// The current user's personal access tokens (metadata only; plaintext
    /// token values are never retrievable after creation).
    async fn my_access_tokens(&self, ctx: &Context<'_>) -> async_graphql::Result<Vec<AccessTokenObject>> {
        let app = ctx.data::<AppContext>()?;
        let req = ctx.data::<RequestContext>()?;
        let Some(claims) = &req.user else {
            return Err(async_graphql::Error::new("unauthenticated"));
        };
        let tokens = entity::prelude::AccessToken::find()
            .filter(entity::access_token::Column::UserId.eq(claims.sub))
            .all(&app.db)
            .await?;
        Ok(tokens.into_iter().map(AccessTokenObject::from).collect())
    }

    /// Lists all users on the instance, paginated. Restricted to site admins.
    async fn admin_list_users(
        &self,
        ctx: &Context<'_>,
        #[graphql(default = 50)] limit: u64,
        #[graphql(default = 0)] offset: u64,
    ) -> async_graphql::Result<Vec<UserObject>> {
        let app = ctx.data::<AppContext>()?;
        let req = ctx.data::<RequestContext>()?;
        let Some(claims) = &req.user else {
            return Err(async_graphql::Error::new("unauthenticated"));
        };
        if !claims.is_admin {
            return Err(async_graphql::Error::new("forbidden"));
        }

        let users = entity::prelude::User::find()
            .order_by_asc(entity::user::Column::CreatedAt)
            .paginate(&app.db, limit.max(1))
            .fetch_page(offset / limit.max(1))
            .await?;
        Ok(users.into_iter().map(UserObject::from).collect())
    }

    /// Notifications addressed to the current user, most recent first.
    async fn my_notifications(
        &self,
        ctx: &Context<'_>,
        unread_only: Option<bool>,
    ) -> async_graphql::Result<Vec<NotificationObject>> {
        let app = ctx.data::<AppContext>()?;
        let req = ctx.data::<RequestContext>()?;
        let Some(claims) = &req.user else {
            return Err(async_graphql::Error::new("unauthenticated"));
        };

        let mut query = entity::prelude::Notification::find()
            .filter(entity::notification::Column::UserId.eq(claims.sub));
        if unread_only.unwrap_or(false) {
            query = query.filter(entity::notification::Column::ReadAt.is_null());
        }

        let notifications = query
            .order_by_desc(entity::notification::Column::CreatedAt)
            .all(&app.db)
            .await?;
        Ok(notifications.into_iter().map(NotificationObject::from).collect())
    }

    /// Recent activity across every repository the current user can see
    /// (owned, collaborated on, or public).
    async fn my_activity(
        &self,
        ctx: &Context<'_>,
        #[graphql(default = 20)] limit: i32,
    ) -> async_graphql::Result<Vec<ActivityEventObject>> {
        let app = ctx.data::<AppContext>()?;
        let req = ctx.data::<RequestContext>()?;
        let Some(claims) = &req.user else {
            return Err(async_graphql::Error::new("unauthenticated"));
        };

        let owned_ids: Vec<Uuid> = entity::prelude::Repository::find()
            .filter(entity::repository::Column::OwnerType.eq("user"))
            .filter(entity::repository::Column::OwnerId.eq(claims.sub))
            .all(&app.db)
            .await?
            .into_iter()
            .map(|r| r.id)
            .collect();

        let collab_ids: Vec<Uuid> = entity::prelude::RepoCollaborator::find()
            .filter(entity::repo_collaborator::Column::UserId.eq(claims.sub))
            .all(&app.db)
            .await?
            .into_iter()
            .map(|c| c.repo_id)
            .collect();

        let public_ids: Vec<Uuid> = entity::prelude::Repository::find()
            .filter(entity::repository::Column::IsPrivate.eq(false))
            .all(&app.db)
            .await?
            .into_iter()
            .map(|r| r.id)
            .collect();

        let mut visible_ids = owned_ids;
        visible_ids.extend(collab_ids);
        visible_ids.extend(public_ids);
        visible_ids.sort();
        visible_ids.dedup();

        if visible_ids.is_empty() {
            return Ok(vec![]);
        }

        let events = entity::prelude::ActivityEvent::find()
            .filter(entity::activity_event::Column::RepoId.is_in(visible_ids))
            .order_by_desc(entity::activity_event::Column::CreatedAt)
            .limit(limit.max(0) as u64)
            .all(&app.db)
            .await?;
        Ok(events.into_iter().map(ActivityEventObject::from).collect())
    }

    /// Packages published by the current authenticated user.
    async fn my_packages(&self, ctx: &Context<'_>) -> async_graphql::Result<Vec<PackageObject>> {
        let app = ctx.data::<AppContext>()?;
        let req = ctx.data::<RequestContext>()?;
        let Some(claims) = &req.user else {
            return Err(async_graphql::Error::new("unauthenticated"));
        };

        let packages = entity::prelude::Package::find()
            .filter(entity::package::Column::OwnerId.eq(claims.sub))
            .order_by_desc(entity::package::Column::CreatedAt)
            .all(&app.db)
            .await?;
        Ok(packages.into_iter().map(PackageObject::from).collect())
    }

    /// Basic ILIKE-based search across repositories, issues, and users.
    /// Each result list is capped at 20 entries.
    async fn search(&self, ctx: &Context<'_>, query: String) -> async_graphql::Result<SearchResults> {
        let app = ctx.data::<AppContext>()?;
        let req = ctx.data::<RequestContext>()?;
        let pattern = format!("%{}%", query);

        // Repositories: public ones, plus ones the current user owns or collaborates on.
        let mut repo_condition = Condition::any().add(entity::repository::Column::IsPrivate.eq(false));
        if let Some(claims) = &req.user {
            let owned_or_collab_ids: Vec<Uuid> = {
                let mut ids: Vec<Uuid> = entity::prelude::Repository::find()
                    .filter(entity::repository::Column::OwnerType.eq("user"))
                    .filter(entity::repository::Column::OwnerId.eq(claims.sub))
                    .all(&app.db)
                    .await?
                    .into_iter()
                    .map(|r| r.id)
                    .collect();
                ids.extend(
                    entity::prelude::RepoCollaborator::find()
                        .filter(entity::repo_collaborator::Column::UserId.eq(claims.sub))
                        .all(&app.db)
                        .await?
                        .into_iter()
                        .map(|c| c.repo_id),
                );
                ids
            };
            if !owned_or_collab_ids.is_empty() {
                repo_condition = repo_condition.add(entity::repository::Column::Id.is_in(owned_or_collab_ids));
            }
        }

        let repos = entity::prelude::Repository::find()
            .filter(repo_condition)
            .filter(
                Condition::any()
                    .add(Expr::col(entity::repository::Column::Name).like(&pattern))
                    .add(Expr::col(entity::repository::Column::Description).like(&pattern)),
            )
            .limit(20)
            .all(&app.db)
            .await?;
        let mut repositories = Vec::with_capacity(repos.len());
        for r in repos {
            repositories.push(RepositoryObject::from_model(&app.db, r).await);
        }

        // Issues: only from repos visible to the current search context (public,
        // or owned/collaborated-on by the current user).
        let visible_repo_ids: Vec<Uuid> = {
            let mut cond = Condition::any().add(entity::repository::Column::IsPrivate.eq(false));
            if let Some(claims) = &req.user {
                let mut ids: Vec<Uuid> = entity::prelude::Repository::find()
                    .filter(entity::repository::Column::OwnerType.eq("user"))
                    .filter(entity::repository::Column::OwnerId.eq(claims.sub))
                    .all(&app.db)
                    .await?
                    .into_iter()
                    .map(|r| r.id)
                    .collect();
                ids.extend(
                    entity::prelude::RepoCollaborator::find()
                        .filter(entity::repo_collaborator::Column::UserId.eq(claims.sub))
                        .all(&app.db)
                        .await?
                        .into_iter()
                        .map(|c| c.repo_id),
                );
                if !ids.is_empty() {
                    cond = cond.add(entity::repository::Column::Id.is_in(ids));
                }
            }
            entity::prelude::Repository::find()
                .filter(cond)
                .all(&app.db)
                .await?
                .into_iter()
                .map(|r| r.id)
                .collect()
        };

        let issues = if visible_repo_ids.is_empty() {
            vec![]
        } else {
            entity::prelude::Issue::find()
                .filter(entity::issue::Column::RepoId.is_in(visible_repo_ids))
                .filter(
                    Condition::any()
                        .add(Expr::col(entity::issue::Column::Title).like(&pattern))
                        .add(Expr::col(entity::issue::Column::Body).like(&pattern)),
                )
                .limit(20)
                .all(&app.db)
                .await?
        };

        let users = entity::prelude::User::find()
            .filter(Expr::col(entity::user::Column::Username).like(&pattern))
            .limit(20)
            .all(&app.db)
            .await?;

        Ok(SearchResults {
            repositories,
            issues: issues.into_iter().map(IssueObject::from).collect(),
            users: users.into_iter().map(UserObject::from).collect(),
        })
    }
}

use async_graphql::{Context, Object};
use hiqlite::params;
use uuid::Uuid;

use crate::context::{AppContext, RequestContext};
use crate::types::{
    AccessTokenObject, ActivityEventObject, CodeSearchResultObject, DevWorkspaceObject, IssueObject,
    NotificationObject, OrganizationObject, PackageObject, RepositoryObject, SearchResults,
    SshKeyObject, UserObject,
};

/// Turns free-text user input into a safe SQLite FTS5 `MATCH` query.
///
/// Each whitespace-separated term becomes either an unquoted `term*` prefix
/// match (when it's plain alphanumeric/underscore -- the common case, and
/// what gives search-as-you-type behavior) or a double-quoted phrase with
/// embedded quotes doubled (FTS5's own escaping rule) for anything else, so
/// user-supplied FTS5 operators/punctuation (`AND`, `"`, `(`, `-`, ...) can
/// never be parsed as query syntax. Terms are ANDed together (FTS5's
/// default). Returns `None` if there are no usable terms, since `MATCH ""`
/// is a syntax error rather than a "match nothing" query.
fn build_fts_match_query(query: &str) -> Option<String> {
    let terms: Vec<String> = query
        .split_whitespace()
        .map(|term| {
            if !term.is_empty() && term.chars().all(|c| c.is_alphanumeric() || c == '_') {
                format!("{term}*")
            } else {
                format!("\"{}\"", term.replace('"', "\"\""))
            }
        })
        .collect();
    if terms.is_empty() {
        None
    } else {
        Some(terms.join(" "))
    }
}

/// Row shape for the `code_search_fts` MATCH query in `search` below --
/// there's no `entity` model for it since it's a pure FTS5 virtual table,
/// not a regular data table with a persisted repository/entity type.
#[derive(serde::Deserialize)]
struct CodeSearchRow {
    repo_id: String,
    path: String,
    snippet: String,
}

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
        let users = app
            .db
            .query_as::<entity::user::Model, _>(
                "SELECT * FROM users WHERE id = ?1",
                params!(claims.sub.to_string()),
            )
            .await?;
        Ok(users.into_iter().next().map(UserObject::from))
    }

    /// The current user's registered SSH public keys (for git-over-SSH auth).
    async fn my_ssh_keys(&self, ctx: &Context<'_>) -> async_graphql::Result<Vec<SshKeyObject>> {
        let app = ctx.data::<AppContext>()?;
        let req = ctx.data::<RequestContext>()?;
        let Some(claims) = &req.user else {
            return Err(async_graphql::Error::new("unauthenticated"));
        };
        let keys = app
            .db
            .query_as::<entity::ssh_key::Model, _>(
                "SELECT * FROM ssh_keys WHERE user_id = ?1",
                params!(claims.sub.to_string()),
            )
            .await?;
        Ok(keys.into_iter().map(SshKeyObject::from).collect())
    }

    /// Look up a user by username.
    async fn user(&self, ctx: &Context<'_>, username: String) -> async_graphql::Result<Option<UserObject>> {
        let app = ctx.data::<AppContext>()?;
        let users = app
            .db
            .query_as::<entity::user::Model, _>(
                "SELECT * FROM users WHERE username = ?1",
                params!(username),
            )
            .await?;
        Ok(users.into_iter().next().map(UserObject::from))
    }

    /// Repositories owned by, or shared with, the current user.
    async fn my_repositories(&self, ctx: &Context<'_>) -> async_graphql::Result<Vec<RepositoryObject>> {
        let app = ctx.data::<AppContext>()?;
        let req = ctx.data::<RequestContext>()?;
        let Some(claims) = &req.user else {
            return Err(async_graphql::Error::new("unauthenticated"));
        };

        let owned = app
            .db
            .query_as::<entity::repository::Model, _>(
                "SELECT * FROM repositories WHERE owner_type = 'user' AND owner_id = ?1",
                params!(claims.sub.to_string()),
            )
            .await?;

        let collab_repo_ids: Vec<Uuid> = app
            .db
            .query_as::<entity::repo_collaborator::Model, _>(
                "SELECT * FROM repo_collaborators WHERE user_id = ?1",
                params!(claims.sub.to_string()),
            )
            .await?
            .into_iter()
            .map(|c| c.repo_id)
            .collect();

        let mut repos = owned;
        if !collab_repo_ids.is_empty() {
            let placeholders: Vec<String> = (1..=collab_repo_ids.len()).map(|i| format!("?{i}")).collect();
            let sql = format!(
                "SELECT * FROM repositories WHERE id IN ({})",
                placeholders.join(", ")
            );
            let ids: Vec<String> = collab_repo_ids.iter().map(|id| id.to_string()).collect();
            let collab_repos = app
                .db
                .query_as::<entity::repository::Model, _>(sql, params_from_strings(ids))
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

        let user = app
            .db
            .query_as::<entity::user::Model, _>(
                "SELECT * FROM users WHERE username = ?1",
                params!(owner.clone()),
            )
            .await?
            .into_iter()
            .next();

        let (owner_type, owner_id) = if let Some(u) = user {
            ("user".to_string(), u.id)
        } else if let Some(org) = app
            .db
            .query_as::<entity::organization::Model, _>(
                "SELECT * FROM organizations WHERE name = ?1",
                params!(owner.clone()),
            )
            .await?
            .into_iter()
            .next()
        {
            ("organization".to_string(), org.id)
        } else {
            return Ok(None);
        };

        let repo = app
            .db
            .query_as::<entity::repository::Model, _>(
                "SELECT * FROM repositories WHERE owner_type = ?1 AND owner_id = ?2 AND name = ?3",
                params!(owner_type, owner_id.to_string(), name),
            )
            .await?
            .into_iter()
            .next();

        match repo {
            Some(r) => Ok(Some(RepositoryObject::from_model(&app.db, r).await)),
            None => Ok(None),
        }
    }

    /// Organizations the current user belongs to (or all, if unauthenticated is not allowed -
    /// here we simply return all organizations, which are not sensitive).
    async fn organizations(&self, ctx: &Context<'_>) -> async_graphql::Result<Vec<OrganizationObject>> {
        let app = ctx.data::<AppContext>()?;
        let orgs = app
            .db
            .query_as::<entity::organization::Model, _>("SELECT * FROM organizations", params!())
            .await?;
        Ok(orgs.into_iter().map(OrganizationObject::from).collect())
    }

    /// Look up an organization by name.
    async fn organization(
        &self,
        ctx: &Context<'_>,
        name: String,
    ) -> async_graphql::Result<Option<OrganizationObject>> {
        let app = ctx.data::<AppContext>()?;
        let org = app
            .db
            .query_as::<entity::organization::Model, _>(
                "SELECT * FROM organizations WHERE name = ?1",
                params!(name),
            )
            .await?
            .into_iter()
            .next();
        Ok(org.map(OrganizationObject::from))
    }

    /// Dev workspaces belonging to the current user.
    async fn dev_workspaces(&self, ctx: &Context<'_>) -> async_graphql::Result<Vec<DevWorkspaceObject>> {
        let app = ctx.data::<AppContext>()?;
        let req = ctx.data::<RequestContext>()?;
        let Some(claims) = &req.user else {
            return Err(async_graphql::Error::new("unauthenticated"));
        };
        let workspaces = app
            .db
            .query_as::<entity::dev_workspace::Model, _>(
                "SELECT * FROM dev_workspaces WHERE owner_id = ?1",
                params!(claims.sub.to_string()),
            )
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
        let workspaces = app
            .db
            .query_as::<entity::dev_workspace::Model, _>("SELECT * FROM dev_workspaces", params!())
            .await?;
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
        let tokens = app
            .db
            .query_as::<entity::access_token::Model, _>(
                "SELECT * FROM access_tokens WHERE user_id = ?1",
                params!(claims.sub.to_string()),
            )
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

        let users = app
            .db
            .query_as::<entity::user::Model, _>(
                "SELECT * FROM users ORDER BY created_at ASC LIMIT ?1 OFFSET ?2",
                params!(limit.max(1) as i64, offset as i64),
            )
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

        let notifications = if unread_only.unwrap_or(false) {
            app.db
                .query_as::<entity::notification::Model, _>(
                    "SELECT * FROM notifications WHERE user_id = ?1 AND read_at IS NULL ORDER BY created_at DESC",
                    params!(claims.sub.to_string()),
                )
                .await?
        } else {
            app.db
                .query_as::<entity::notification::Model, _>(
                    "SELECT * FROM notifications WHERE user_id = ?1 ORDER BY created_at DESC",
                    params!(claims.sub.to_string()),
                )
                .await?
        };
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

        let owned_ids: Vec<Uuid> = app
            .db
            .query_as::<entity::repository::Model, _>(
                "SELECT * FROM repositories WHERE owner_type = 'user' AND owner_id = ?1",
                params!(claims.sub.to_string()),
            )
            .await?
            .into_iter()
            .map(|r| r.id)
            .collect();

        let collab_ids: Vec<Uuid> = app
            .db
            .query_as::<entity::repo_collaborator::Model, _>(
                "SELECT * FROM repo_collaborators WHERE user_id = ?1",
                params!(claims.sub.to_string()),
            )
            .await?
            .into_iter()
            .map(|c| c.repo_id)
            .collect();

        let public_ids: Vec<Uuid> = app
            .db
            .query_as::<entity::repository::Model, _>(
                "SELECT * FROM repositories WHERE is_private = 0",
                params!(),
            )
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

        let placeholders: Vec<String> = (1..=visible_ids.len()).map(|i| format!("?{i}")).collect();
        let sql = format!(
            "SELECT * FROM activity_events WHERE repo_id IN ({}) ORDER BY created_at DESC LIMIT ?{}",
            placeholders.join(", "),
            visible_ids.len() + 1
        );
        let mut string_ids: Vec<String> = visible_ids.iter().map(|id| id.to_string()).collect();
        let mut params_vec = params_from_strings_owned(&string_ids);
        params_vec.push(hiqlite::Param::Integer(limit.max(0) as i64));
        string_ids.clear();
        let events = app
            .db
            .query_as::<entity::activity_event::Model, _>(sql, params_vec)
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

        let packages = app
            .db
            .query_as::<entity::package::Model, _>(
                "SELECT * FROM packages WHERE owner_id = ?1 ORDER BY created_at DESC",
                params!(claims.sub.to_string()),
            )
            .await?;
        Ok(packages.into_iter().map(PackageObject::from).collect())
    }

    /// Real full-text search (SQLite FTS5) across repositories, issues,
    /// users, and indexed file content. Each result list is capped at 20
    /// entries. Falls back to returning empty results (rather than an
    /// error) when `query` has no usable search terms (e.g. only
    /// punctuation/whitespace).
    async fn search(&self, ctx: &Context<'_>, query: String) -> async_graphql::Result<SearchResults> {
        let app = ctx.data::<AppContext>()?;
        let req = ctx.data::<RequestContext>()?;

        let Some(fts_query) = build_fts_match_query(&query) else {
            return Ok(SearchResults {
                repositories: vec![],
                issues: vec![],
                users: vec![],
                code: vec![],
            });
        };

        // Repositories: public ones, plus ones the current user owns or collaborates on.
        let mut owned_or_collab_ids: Vec<Uuid> = vec![];
        if let Some(claims) = &req.user {
            owned_or_collab_ids = app
                .db
                .query_as::<entity::repository::Model, _>(
                    "SELECT * FROM repositories WHERE owner_type = 'user' AND owner_id = ?1",
                    params!(claims.sub.to_string()),
                )
                .await?
                .into_iter()
                .map(|r| r.id)
                .collect();
            owned_or_collab_ids.extend(
                app.db
                    .query_as::<entity::repo_collaborator::Model, _>(
                        "SELECT * FROM repo_collaborators WHERE user_id = ?1",
                        params!(claims.sub.to_string()),
                    )
                    .await?
                    .into_iter()
                    .map(|c| c.repo_id),
            );
        }

        let repo_sql = if owned_or_collab_ids.is_empty() {
            "SELECT r.* FROM repositories r \
             JOIN repositories_fts ON repositories_fts.id = r.id \
             WHERE repositories_fts MATCH ?1 AND r.is_private = 0 \
             ORDER BY repositories_fts.rank LIMIT 20"
                .to_string()
        } else {
            let placeholders: Vec<String> = (2..=owned_or_collab_ids.len() + 1)
                .map(|i| format!("?{i}"))
                .collect();
            format!(
                "SELECT r.* FROM repositories r \
                 JOIN repositories_fts ON repositories_fts.id = r.id \
                 WHERE repositories_fts MATCH ?1 AND (r.is_private = 0 OR r.id IN ({})) \
                 ORDER BY repositories_fts.rank LIMIT 20",
                placeholders.join(", ")
            )
        };
        let mut repo_params = vec![hiqlite::Param::Text(fts_query.clone())];
        for id in &owned_or_collab_ids {
            repo_params.push(hiqlite::Param::Text(id.to_string()));
        }
        let repos = app
            .db
            .query_as::<entity::repository::Model, _>(repo_sql, repo_params)
            .await?;
        let mut repositories = Vec::with_capacity(repos.len());
        for r in repos {
            repositories.push(RepositoryObject::from_model(&app.db, r).await);
        }

        // Issues + code: only from repos visible to the current search context
        // (public, or owned/collaborated-on by the current user).
        let visible_repo_sql = if owned_or_collab_ids.is_empty() {
            "SELECT * FROM repositories WHERE is_private = 0".to_string()
        } else {
            let placeholders: Vec<String> = (1..=owned_or_collab_ids.len()).map(|i| format!("?{i}")).collect();
            format!(
                "SELECT * FROM repositories WHERE is_private = 0 OR id IN ({})",
                placeholders.join(", ")
            )
        };
        let visible_repo_params: Vec<hiqlite::Param> = owned_or_collab_ids
            .iter()
            .map(|id| hiqlite::Param::Text(id.to_string()))
            .collect();
        let visible_repos = app
            .db
            .query_as::<entity::repository::Model, _>(visible_repo_sql, visible_repo_params)
            .await?;
        let visible_repo_ids: Vec<Uuid> = visible_repos.iter().map(|r| r.id).collect();

        let issues = if visible_repo_ids.is_empty() {
            vec![]
        } else {
            let placeholders: Vec<String> = (2..=visible_repo_ids.len() + 1).map(|i| format!("?{i}")).collect();
            let sql = format!(
                "SELECT i.* FROM issues i \
                 JOIN issues_fts ON issues_fts.id = i.id \
                 WHERE issues_fts MATCH ?1 AND i.repo_id IN ({}) \
                 ORDER BY issues_fts.rank LIMIT 20",
                placeholders.join(", ")
            );
            let mut p = vec![hiqlite::Param::Text(fts_query.clone())];
            for id in &visible_repo_ids {
                p.push(hiqlite::Param::Text(id.to_string()));
            }
            app.db.query_as::<entity::issue::Model, _>(sql, p).await?
        };

        let code = if visible_repo_ids.is_empty() {
            vec![]
        } else {
            let placeholders: Vec<String> = (2..=visible_repo_ids.len() + 1).map(|i| format!("?{i}")).collect();
            let sql = format!(
                "SELECT repo_id, path, \
                     snippet(code_search_fts, 2, '[b]', '[/b]', '...', 12) AS snippet \
                 FROM code_search_fts \
                 WHERE code_search_fts MATCH ?1 AND repo_id IN ({}) \
                 ORDER BY rank LIMIT 20",
                placeholders.join(", ")
            );
            let mut p = vec![hiqlite::Param::Text(fts_query.clone())];
            for id in &visible_repo_ids {
                p.push(hiqlite::Param::Text(id.to_string()));
            }
            app.db
                .query_as::<CodeSearchRow, _>(sql, p)
                .await?
        };
        let repos_by_id: std::collections::HashMap<Uuid, entity::repository::Model> =
            visible_repos.into_iter().map(|r| (r.id, r)).collect();
        let mut code_results = Vec::with_capacity(code.len());
        for row in code {
            let Ok(repo_id) = row.repo_id.parse::<Uuid>() else {
                continue;
            };
            let Some(repo) = repos_by_id.get(&repo_id).cloned() else {
                continue;
            };
            code_results.push(CodeSearchResultObject {
                repository: RepositoryObject::from_model(&app.db, repo).await,
                path: row.path,
                snippet: row.snippet,
            });
        }

        let users = app
            .db
            .query_as::<entity::user::Model, _>(
                "SELECT u.* FROM users u \
                 JOIN users_fts ON users_fts.id = u.id \
                 WHERE users_fts MATCH ?1 ORDER BY users_fts.rank LIMIT 20",
                params!(fts_query),
            )
            .await?;

        Ok(SearchResults {
            repositories,
            issues: issues.into_iter().map(IssueObject::from).collect(),
            users: users.into_iter().map(UserObject::from).collect(),
            code: code_results,
        })
    }
}

/// Helper: build a `Vec<hiqlite::Param>` of `Param::Text` from owned strings,
/// used for dynamic `IN (...)` clauses where the placeholder count varies.
fn params_from_strings(strings: Vec<String>) -> Vec<hiqlite::Param> {
    strings.into_iter().map(hiqlite::Param::Text).collect()
}

fn params_from_strings_owned(strings: &[String]) -> Vec<hiqlite::Param> {
    strings.iter().map(|s| hiqlite::Param::Text(s.clone())).collect()
}

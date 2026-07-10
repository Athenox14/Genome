mod config;
mod error;
mod rate_limit;

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::{Path as AxumPath, Query, Request, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::middleware;
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::Router;
use migration::MigratorTrait;
use sea_orm::{ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, Set};
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;
use uuid::Uuid;

use config::Config;
use error::{ServerError, ServerResult};
use graphql_api::{AppContext, GraphQLSchema};

#[derive(Clone)]
struct ServerState {
    schema: GraphQLSchema,
    app_ctx: AppContext,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let config = Config::from_env()?;

    let db: DatabaseConnection = sea_orm::Database::connect(&config.database_url).await?;
    migration::Migrator::up(&db, None).await?;

    let repo_manager = Arc::new(git_core::RepoManager::new(config.repos_root_path.clone()));
    let actions_executor = Arc::new(actions::Executor::new()?);
    let workspace_manager = Arc::new(dev_env::WorkspaceManager::connect_local()?);
    let webhook_dispatcher = Arc::new(webhooks::WebhookDispatcher::new(db.clone()));

    let app_ctx = AppContext {
        db: db.clone(),
        repo_manager: repo_manager.clone(),
        jwt_secret: config.jwt_secret.clone(),
        actions_executor: actions_executor.clone(),
        workspace_manager: workspace_manager.clone(),
        webhook_dispatcher: webhook_dispatcher.clone(),
    };

    let schema = graphql_api::build_schema(app_ctx.clone());

    let state = ServerState { schema, app_ctx: app_ctx.clone() };
    let limiter = rate_limit::RateLimiter::new();

    tokio::spawn(auto_stop_dev_workspaces(app_ctx));

    // CORS is intentionally permissive (the frontend runs on a different
    // port/origin by design). `CorsLayer::permissive()` allows any origin,
    // method, and header, but does NOT call `.allow_credentials(true)` — so
    // the insecure "wildcard origin + credentials" combination (invalid per
    // the Fetch spec) is never enabled. Do not add `.allow_credentials(true)`
    // to this layer without also restricting `allow_origin` to a fixed list.
    let cors = CorsLayer::permissive();

    let app = Router::new()
        .route(
            "/graphql",
            get(graphql_playground).post(graphql_post_handler),
        )
        .route("/health", get(health_handler))
        .route(
            "/:owner/:repo/info/refs",
            get(info_refs_handler),
        )
        .route(
            "/:owner/:repo/git-upload-pack",
            post(upload_pack_handler),
        )
        .route(
            "/:owner/:repo/git-receive-pack",
            post(receive_pack_handler),
        )
        .route(
            "/workspaces/:id/proxy/*path",
            get(workspace_proxy_handler).post(workspace_proxy_handler),
        )
        .layer(middleware::from_fn_with_state(
            limiter,
            rate_limit::rate_limit_middleware,
        ))
        .layer(TraceLayer::new_for_http())
        .layer(cors)
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(&config.listen_addr).await?;
    tracing::info!("listening on {}", config.listen_addr);

    let ssh_state = Arc::new(ssh_server::SharedState {
        db: db.clone(),
        repo_manager: repo_manager.clone(),
    });
    let ssh_config = ssh_server::SshServerConfig {
        listen_addr: config.ssh_listen_addr.clone(),
        host_key_path: std::path::PathBuf::from(&config.ssh_host_key_path),
    };

    let http_task = tokio::spawn(async move {
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
    });
    let ssh_task = tokio::spawn(async move { ssh_server::run(ssh_config, ssh_state).await });

    let (http_result, ssh_result) = tokio::join!(http_task, ssh_task);
    http_result??;
    if let Err(e) = ssh_result? {
        tracing::error!("SSH server exited with error: {e}");
    }

    Ok(())
}

async fn health_handler() -> &'static str {
    "ok"
}

async fn graphql_playground() -> impl IntoResponse {
    Html(async_graphql::http::playground_source(
        async_graphql::http::GraphQLPlaygroundConfig::new("/graphql"),
    ))
}

/// Handles `POST /graphql` directly against the `async-graphql` schema.
///
/// Note: `async-graphql-axum`'s `GraphQLRequest`/`GraphQLResponse` extractors
/// are built against a newer `axum` (0.8.x) than the rest of this workspace
/// (0.7.x, per `graphql-api`'s `Handler` impl requirements), so routing
/// directly through them here causes a `Handler` trait mismatch. Instead we
/// deserialize the GraphQL request body ourselves and call
/// `schema.execute()`, matching what `graphql_api::graphql_handler` does
/// internally (auth header extraction + per-request `RequestContext`).
async fn graphql_post_handler(
    State(state): State<ServerState>,
    headers: HeaderMap,
    axum::Json(gql_request): axum::Json<async_graphql::Request>,
) -> axum::Json<async_graphql::Response> {
    let user =
        auth::extract_user_from_headers(&headers, &state.app_ctx.jwt_secret, None).await;
    let request_ctx = graphql_api::RequestContext { user };
    let request = gql_request.data(request_ctx);
    let response = state.schema.execute(request).await;
    axum::Json(response)
}

// ---------------------------------------------------------------------
// Git smart HTTP
// ---------------------------------------------------------------------

async fn info_refs_handler(
    State(state): State<ServerState>,
    AxumPath((owner, repo)): AxumPath<(String, String)>,
    Query(params): Query<HashMap<String, String>>,
) -> ServerResult<Response> {
    let repo_name = strip_git_suffix(&repo);
    let service_param = params
        .get("service")
        .ok_or_else(|| ServerError::BadRequest("missing service query parameter".to_string()))?;
    let service = git_core::GitService::parse(service_param)
        .map_err(|e| ServerError::BadRequest(e.to_string()))?;

    let repo_path = state
        .app_ctx
        .repo_manager
        .repo_path(&owner, repo_name)?;

    let body = git_core::handle_info_refs(&repo_path, service)
        .await
        .map_err(ServerError::Git)?;

    let content_type = git_core::info_refs_content_type(service);
    Ok((
        StatusCode::OK,
        [(header::CONTENT_TYPE, content_type)],
        body,
    )
        .into_response())
}

async fn upload_pack_handler(
    State(state): State<ServerState>,
    AxumPath((owner, repo)): AxumPath<(String, String)>,
    body: Bytes,
) -> ServerResult<Response> {
    let repo_name = strip_git_suffix(&repo);
    let repo_path = state
        .app_ctx
        .repo_manager
        .repo_path(&owner, repo_name)?;

    let result =
        git_core::handle_service_rpc(&repo_path, git_core::GitService::UploadPack, body)
            .await
            .map_err(ServerError::Git)?;

    let content_type =
        git_core::service_rpc_content_type(git_core::GitService::UploadPack);
    Ok((
        StatusCode::OK,
        [(header::CONTENT_TYPE, content_type)],
        result,
    )
        .into_response())
}

async fn receive_pack_handler(
    State(state): State<ServerState>,
    AxumPath((owner, repo)): AxumPath<(String, String)>,
    body: Bytes,
) -> ServerResult<Response> {
    let repo_name = strip_git_suffix(&repo).to_string();
    let repo_path = state
        .app_ctx
        .repo_manager
        .repo_path(&owner, &repo_name)?;

    let before = git_core::diff_refs(&repo_path).unwrap_or_default();

    let result =
        git_core::handle_service_rpc(&repo_path, git_core::GitService::ReceivePack, body)
            .await
            .map_err(ServerError::Git)?;

    let after = git_core::diff_refs(&repo_path).unwrap_or_default();
    let changes = git_core::compute_ref_changes(&before, &after);

    if !changes.is_empty() {
        let app_ctx = state.app_ctx.clone();
        let owner_clone = owner.clone();
        let repo_clone = repo_name.clone();
        let changes_clone = changes.clone();
        tokio::spawn(async move {
            if let Err(e) =
                process_push_workflows(app_ctx, owner_clone, repo_clone, changes_clone).await
            {
                tracing::warn!("post-push workflow processing failed: {e}");
            }
        });

        let app_ctx = state.app_ctx.clone();
        let owner_clone = owner.clone();
        let repo_clone = repo_name.clone();
        tokio::spawn(async move {
            check_force_push_against_protection(app_ctx, owner_clone, repo_clone, changes).await;
        });
    }

    let content_type =
        git_core::service_rpc_content_type(git_core::GitService::ReceivePack);
    Ok((
        StatusCode::OK,
        [(header::CONTENT_TYPE, content_type)],
        result,
    )
        .into_response())
}

/// Fire-and-forget task run after a successful `git-receive-pack`: discovers
/// `.github/workflows/*.yml` files in the pushed branch tips, matches them
/// against the `push` event, runs matching jobs, and records a
/// `workflow_run` row for each.
async fn process_push_workflows(
    app_ctx: AppContext,
    owner: String,
    repo: String,
    changes: Vec<(String, String, String)>,
) -> anyhow::Result<()> {
    const ZERO_SHA: &str = "0000000000000000000000000000000000000000";

    let repo_row = entity::prelude::Repository::find()
        .filter(entity::repository::Column::Name.eq(repo.clone()))
        .one(&app_ctx.db)
        .await?;

    let Some(repo_row) = repo_row else {
        tracing::warn!("no repository row found for {owner}/{repo}; skipping workflow trigger");
        return Ok(());
    };

    let secrets = load_repo_secrets(&app_ctx, repo_row.id).await.unwrap_or_default();

    for (ref_name, old_sha, new_sha) in changes {
        if new_sha == ZERO_SHA {
            // Branch deletion: nothing to run.
            continue;
        }

        let push_payload = serde_json::json!({
            "ref": ref_name,
            "before": old_sha,
            "after": new_sha,
        });

        // Best-effort activity feed entry for the push. The pusher's identity
        // isn't threaded through git's smart-HTTP handlers here, so we
        // attribute the push to the repository owner account when it's a
        // user-owned repo (organization-owned pushes are skipped rather than
        // guessing at an actor).
        if repo_row.owner_type == "user" {
            let event = entity::activity_event::ActiveModel {
                id: Set(Uuid::new_v4()),
                repo_id: Set(Some(repo_row.id)),
                actor_id: Set(repo_row.owner_id),
                kind: Set(entity::activity_event::kind::PUSH.to_string()),
                summary: Set(format!("push to {ref_name} on {owner}/{repo}")),
                created_at: Set(chrono::Utc::now()),
            };
            if let Err(e) = event.insert(&app_ctx.db).await {
                tracing::warn!("failed to insert activity event for push: {e}");
            }
        }

        let _ = app_ctx
            .webhook_dispatcher
            .dispatch(repo_row.id, "push", push_payload)
            .await;

        let Some(branch) = ref_name.strip_prefix("refs/heads/") else {
            continue;
        };

        let entries = match app_ctx
            .repo_manager
            .list_tree(&owner, &repo, &new_sha, ".github/workflows")
        {
            Ok(entries) => entries,
            Err(_) => continue, // no workflows directory at this ref
        };

        let mut repo_files: HashMap<String, Vec<u8>> = HashMap::new();
        for entry in entries {
            if entry.kind != git_core::EntryKind::Blob {
                continue;
            }
            if let Ok(contents) =
                app_ctx
                    .repo_manager
                    .read_file_at_ref(&owner, &repo, &new_sha, &entry.path)
            {
                repo_files.insert(entry.path.clone(), contents);
            }
        }

        let workflows = actions::discover_workflows(&repo_files);

        for (path, workflow) in workflows {
            if !actions::matches_event(&workflow.on, "push", branch) {
                continue;
            }

            let workflow_name = workflow.name.clone().unwrap_or_else(|| path.clone());

            let run = entity::workflow_run::ActiveModel {
                id: Set(Uuid::new_v4()),
                repo_id: Set(repo_row.id),
                workflow_name: Set(workflow_name.clone()),
                commit_sha: Set(new_sha.clone()),
                event: Set("push".to_string()),
                status: Set(entity::workflow_run::status::RUNNING.to_string()),
                started_at: Set(Some(chrono::Utc::now())),
                finished_at: Set(None),
            };
            let inserted = run.insert(&app_ctx.db).await;
            let run_model = match inserted {
                Ok(m) => m,
                Err(e) => {
                    tracing::warn!("failed to record workflow_run: {e}");
                    continue;
                }
            };

            let archive = match app_ctx
                .repo_manager
                .archive_tree_at_ref(&owner, &repo, &new_sha)
            {
                Ok(archive) => archive,
                Err(e) => {
                    tracing::warn!(
                        "failed to archive tree for {owner}/{repo}@{new_sha}: {e}"
                    );
                    continue;
                }
            };

            for (_job_id, job) in workflow.jobs.iter() {
                let env_extra = HashMap::new();
                let executor = app_ctx.actions_executor.clone();
                let job = job.clone();
                let archive = archive.clone();
                let result = executor
                    .run_job(&job, &archive, env_extra, &secrets, |line| {
                        tracing::info!("[workflow] {line}");
                    })
                    .await;

                let status = match result {
                    Ok(job_result) => match job_result.status {
                        actions::JobStatus::Success => entity::workflow_run::status::SUCCESS,
                        actions::JobStatus::Failure => entity::workflow_run::status::FAILURE,
                    },
                    Err(e) => {
                        tracing::warn!("job execution failed: {e}");
                        entity::workflow_run::status::FAILURE
                    }
                };

                let mut update: entity::workflow_run::ActiveModel = run_model.clone().into();
                update.status = Set(status.to_string());
                update.finished_at = Set(Some(chrono::Utc::now()));
                if let Err(e) = update.update(&app_ctx.db).await {
                    tracing::warn!("failed to update workflow_run: {e}");
                }
            }
        }
    }

    Ok(())
}

/// Best-effort force-push detection for protected branches.
///
/// By the time this runs, `git-receive-pack` has already accepted the push
/// (git's real force-push rejection happens in a pre-receive hook, which
/// would require running a hook binary as part of `handle_service_rpc` — a
/// larger change than warranted here). Instead we compare each updated ref's
/// old/new tips: if the old tip is not an ancestor of the new tip, history
/// was rewritten (a force push). If that ref is a branch matching an active
/// `branch_protection_rules` row with `block_force_push` set, we log a
/// warning so operators/audits can see the violation. This does not undo or
/// block the push.
async fn check_force_push_against_protection(
    app_ctx: AppContext,
    owner: String,
    repo: String,
    changes: Vec<(String, String, String)>,
) {
    const ZERO_SHA: &str = "0000000000000000000000000000000000000000";

    let repo_row = match entity::prelude::Repository::find()
        .filter(entity::repository::Column::Name.eq(repo.clone()))
        .one(&app_ctx.db)
        .await
    {
        Ok(Some(r)) => r,
        Ok(None) => return,
        Err(e) => {
            tracing::warn!("branch protection check: failed to load repository row: {e}");
            return;
        }
    };

    let rules = match entity::prelude::BranchProtectionRule::find()
        .filter(entity::branch_protection_rule::Column::RepoId.eq(repo_row.id))
        .all(&app_ctx.db)
        .await
    {
        Ok(rules) => rules,
        Err(e) => {
            tracing::warn!("branch protection check: failed to load rules: {e}");
            return;
        }
    };
    if rules.is_empty() {
        return;
    }

    for (ref_name, old_sha, new_sha) in changes {
        if old_sha == ZERO_SHA || new_sha == ZERO_SHA {
            // Branch creation or deletion, not a force-push.
            continue;
        }
        let Some(branch) = ref_name.strip_prefix("refs/heads/") else {
            continue;
        };

        let Some(rule) = rules
            .iter()
            .find(|r| entity::branch_protection_rule::branch_matches_pattern(&r.branch_pattern, branch))
        else {
            continue;
        };
        if !rule.block_force_push {
            continue;
        }

        match app_ctx.repo_manager.is_ancestor(&owner, &repo, &old_sha, &new_sha) {
            Ok(true) => {} // fast-forward, fine
            Ok(false) => {
                tracing::warn!(
                    "force push detected on protected branch '{branch}' of {owner}/{repo} \
                     ({old_sha} -> {new_sha}); block_force_push is set but was not enforced \
                     pre-receive (see check_force_push_against_protection doc comment)"
                );
            }
            Err(e) => {
                tracing::warn!("branch protection check: failed to determine ancestry for {owner}/{repo} {branch}: {e}");
            }
        }
    }
}

fn strip_git_suffix(name: &str) -> &str {
    name.strip_suffix(".git").unwrap_or(name)
}

/// Loads and decrypts all Actions secrets for a repository, for injection
/// into workflow job runs triggered by a push. Best-effort: if
/// `SECRETS_ENCRYPTION_KEY` is unset or a value fails to decrypt, that
/// secret (or all secrets) is skipped with a warning rather than aborting
/// the workflow run.
async fn load_repo_secrets(
    app_ctx: &AppContext,
    repo_id: Uuid,
) -> anyhow::Result<HashMap<String, String>> {
    let encoded = std::env::var("SECRETS_ENCRYPTION_KEY")
        .map_err(|_| anyhow::anyhow!("SECRETS_ENCRYPTION_KEY environment variable must be set"))?;
    let key = actions::key_from_base64(&encoded)?;

    let rows = entity::prelude::RepoSecret::find()
        .filter(entity::repo_secret::Column::RepoId.eq(repo_id))
        .all(&app_ctx.db)
        .await?;

    let mut out = HashMap::new();
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

/// Background task: every 60s, stop any running dev workspace whose
/// `auto_stop_minutes` timer has elapsed since `last_activity_at`.
async fn auto_stop_dev_workspaces(app_ctx: AppContext) {
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
    loop {
        interval.tick().await;

        let workspaces = match entity::prelude::DevWorkspace::find()
            .filter(entity::dev_workspace::Column::Status.eq(entity::dev_workspace::status::RUNNING))
            .all(&app_ctx.db)
            .await
        {
            Ok(w) => w,
            Err(e) => {
                tracing::warn!("auto-stop: failed to list dev workspaces: {e}");
                continue;
            }
        };

        let now = chrono::Utc::now();
        for workspace in workspaces {
            let (Some(auto_stop_minutes), Some(last_activity_at)) =
                (workspace.auto_stop_minutes, workspace.last_activity_at)
            else {
                continue;
            };
            let deadline = last_activity_at + chrono::Duration::minutes(auto_stop_minutes as i64);
            if deadline >= now {
                continue;
            }

            let Some(container_id) = workspace.container_id.clone() else {
                continue;
            };

            if let Err(e) = app_ctx.workspace_manager.stop_workspace(&container_id).await {
                tracing::warn!("auto-stop: failed to stop workspace {}: {e}", workspace.id);
                continue;
            }

            let mut active: entity::dev_workspace::ActiveModel = workspace.into();
            active.status = Set(entity::dev_workspace::status::STOPPED.to_string());
            if let Err(e) = active.update(&app_ctx.db).await {
                tracing::warn!("auto-stop: failed to update workspace status: {e}");
            }
        }
    }
}

// ---------------------------------------------------------------------
// Dev workspace proxy
// ---------------------------------------------------------------------

async fn workspace_proxy_handler(
    State(state): State<ServerState>,
    AxumPath((id, path)): AxumPath<(String, String)>,
    req: Request,
) -> ServerResult<Response> {
    let workspace_id = Uuid::parse_str(&id)
        .map_err(|_| ServerError::BadRequest("invalid workspace id".to_string()))?;

    let workspace = entity::prelude::DevWorkspace::find_by_id(workspace_id)
        .one(&state.app_ctx.db)
        .await?
        .ok_or_else(|| ServerError::NotFound(format!("workspace {id} not found")))?;

    let host_port = workspace
        .container_id
        .as_ref()
        .and_then(|_| None::<u16>)
        .unwrap_or(0);

    // The dev_workspace row does not store the host port directly; it is
    // only known at container-creation time via WorkspaceHandle. We look
    // it up live from Docker instead.
    let _ = host_port;
    let container_id = workspace
        .container_id
        .ok_or_else(|| ServerError::BadRequest("workspace has no container".to_string()))?;

    let host_port = resolve_host_port(&state.app_ctx.workspace_manager, &container_id).await?;

    // Best-effort activity bump: never block the proxied request on this.
    {
        let db = state.app_ctx.db.clone();
        tokio::spawn(async move {
            let mut active = entity::dev_workspace::ActiveModel {
                id: Set(workspace_id),
                ..Default::default()
            };
            active.last_activity_at = Set(Some(chrono::Utc::now()));
            if let Err(e) = active.update(&db).await {
                tracing::debug!("failed to bump workspace last_activity_at: {e}");
            }
        });
    }

    let req_path = format!("/{path}");
    let response = dev_env::proxy::proxy_to_workspace(host_port, &req_path, req)
        .await
        .map_err(ServerError::DevEnv)?;

    Ok(response)
}

async fn resolve_host_port(
    workspace_manager: &dev_env::WorkspaceManager,
    container_id: &str,
) -> ServerResult<u16> {
    let handles = workspace_manager
        .list_workspaces("")
        .await
        .map_err(ServerError::DevEnv)?;
    handles
        .into_iter()
        .find(|h| h.container_id == container_id)
        .map(|h| h.host_port)
        .ok_or_else(|| ServerError::NotFound("workspace container not found".to_string()))
}

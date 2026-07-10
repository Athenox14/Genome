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
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::routing::{get, post, put};
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
    config: Config,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // `check-push-protection` is not the HTTP server: it's a tiny CLI
    // subcommand invoked by the `hooks/pre-receive` script that
    // `git_core::RepoManager::init_repo` writes into every new bare repo.
    // git runs this hook *before* updating any refs and feeds it
    // `<old> <new> <ref>` lines on stdin for the push; a non-zero exit here
    // aborts the whole push (true pre-receive rejection), unlike the old
    // post-receive warning-only check.
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("check-push-protection") {
        let exit_code = run_check_push_protection(&args[2..]).await?;
        std::process::exit(exit_code);
    }

    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let config = Config::from_env()?;

    let db: DatabaseConnection = sea_orm::Database::connect(&config.database_url).await?;
    migration::Migrator::up(&db, None).await?;

    let repo_manager = Arc::new(git_core::RepoManager::new(config.repos_root_path.clone()));
    let artifacts_root = std::path::Path::new(&config.repos_root_path)
        .parent()
        .map(|p| p.join("artifacts"))
        .unwrap_or_else(|| std::path::PathBuf::from("./artifacts"));
    let actions_executor = Arc::new(actions::Executor::new(artifacts_root)?);
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

    let state = ServerState {
        schema,
        app_ctx: app_ctx.clone(),
        config: config.clone(),
    };
    let limiter = rate_limit::RateLimiter::new();

    tokio::spawn(auto_stop_dev_workspaces(app_ctx.clone()));
    tokio::spawn(sync_repo_mirrors(app_ctx));

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
        .route(
            "/oauth/authorize",
            get(oauth_authorize_handler),
        )
        .route("/oauth/token", post(oauth_token_handler))
        .route(
            "/packages/:owner/:name/:version",
            put(package_upload_handler).get(package_download_handler),
        )
        .route(
            "/artifacts/:id/download",
            get(download_artifact_handler),
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
                    .run_job(run_model.id, &job, &archive, env_extra, &secrets, |line| {
                        tracing::info!("[workflow] {line}");
                    })
                    .await;

                let status = match result {
                    Ok(job_result) => {
                        for artifact in &job_result.artifacts {
                            let row = entity::workflow_artifact::ActiveModel {
                                id: Set(Uuid::new_v4()),
                                run_id: Set(run_model.id),
                                job_id: Set(None),
                                name: Set(artifact.name.clone()),
                                file_path: Set(artifact.file_path.clone()),
                                size_bytes: Set(artifact.size_bytes),
                                created_at: Set(chrono::Utc::now()),
                            };
                            if let Err(e) = row.insert(&app_ctx.db).await {
                                tracing::warn!("failed to insert workflow_artifact: {e}");
                            }
                        }
                        match job_result.status {
                            actions::JobStatus::Success => entity::workflow_run::status::SUCCESS,
                            actions::JobStatus::Failure => entity::workflow_run::status::FAILURE,
                        }
                    }
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

/// The `check-push-protection` CLI subcommand, invoked by the
/// `hooks/pre-receive` script written into every bare repo by
/// `git_core::RepoManager::init_repo`. Git runs this *before* updating any
/// refs and pipes one `<old-sha> <new-sha> <ref-name>` line per updated ref
/// into our stdin. Returning a non-zero exit code here makes git abort the
/// entire push -- no refs are updated -- which is true pre-receive
/// rejection, unlike the old post-receive warning that ran after
/// `git-receive-pack` had already accepted the push.
///
/// Expected args (order-independent): `--repo-path <path> --owner <owner>
/// --name <name>`.
///
/// SCOPE NOTE: only repos created after this change ships have the hook
/// installed (see the doc comment on `write_pre_receive_hook` in
/// `git-core`); pre-existing repos are not retroactively protected.
async fn run_check_push_protection(args: &[String]) -> anyhow::Result<i32> {
    let mut repo_path: Option<std::path::PathBuf> = None;
    let mut owner: Option<String> = None;
    let mut name: Option<String> = None;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--repo-path" if i + 1 < args.len() => {
                repo_path = Some(std::path::PathBuf::from(&args[i + 1]));
                i += 2;
            }
            "--owner" if i + 1 < args.len() => {
                owner = Some(args[i + 1].clone());
                i += 2;
            }
            "--name" if i + 1 < args.len() => {
                name = Some(args[i + 1].clone());
                i += 2;
            }
            _ => {
                i += 1;
            }
        }
    }

    let (Some(repo_path), Some(owner), Some(name)) = (repo_path, owner, name) else {
        eprintln!(
            "check-push-protection: usage: check-push-protection --repo-path <path> --owner <owner> --name <name>"
        );
        return Ok(2);
    };

    // Read all `<old> <new> <ref>` lines from stdin (git feeds every updated
    // ref for this push before we're expected to answer).
    let mut stdin_buf = String::new();
    std::io::Read::read_to_string(&mut std::io::stdin(), &mut stdin_buf)?;
    let updates: Vec<(String, String, String)> = stdin_buf
        .lines()
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let old_sha = parts.next()?;
            let new_sha = parts.next()?;
            let ref_name = parts.next()?;
            Some((old_sha.to_string(), new_sha.to_string(), ref_name.to_string()))
        })
        .collect();

    if updates.is_empty() {
        return Ok(0);
    }

    const ZERO_SHA: &str = "0000000000000000000000000000000000000000";

    let database_url = std::env::var("DATABASE_URL")
        .map_err(|_| anyhow::anyhow!("DATABASE_URL environment variable must be set"))?;
    let db: DatabaseConnection = sea_orm::Database::connect(&database_url).await?;

    let repo_row = entity::prelude::Repository::find()
        .filter(entity::repository::Column::Name.eq(name.clone()))
        .one(&db)
        .await?;

    let Some(repo_row) = repo_row else {
        // No matching repository row (shouldn't normally happen for a repo
        // that has a hook at all); fail open rather than blocking pushes.
        return Ok(0);
    };

    let rules = entity::prelude::BranchProtectionRule::find()
        .filter(entity::branch_protection_rule::Column::RepoId.eq(repo_row.id))
        .all(&db)
        .await?;

    if rules.is_empty() {
        return Ok(0);
    }

    // `RepoManager::is_ancestor` resolves `{root}/{owner}/{name}.git` itself,
    // so derive `root` from the concrete `repo_path` we were given
    // (`root/owner/name.git` -> `root`).
    let root = repo_path
        .parent()
        .and_then(|p| p.parent())
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    let repo_manager = git_core::RepoManager::new(root);

    let mut rejected = false;
    for (old_sha, new_sha, ref_name) in updates {
        if old_sha == ZERO_SHA || new_sha == ZERO_SHA {
            // Branch creation or deletion: not a force-push.
            continue;
        }
        let Some(branch) = ref_name.strip_prefix("refs/heads/") else {
            continue;
        };

        let Some(rule) = rules.iter().find(|r| {
            entity::branch_protection_rule::branch_matches_pattern(&r.branch_pattern, branch)
        }) else {
            continue;
        };
        if !rule.block_force_push {
            continue;
        }

        match repo_manager.is_ancestor(&owner, &name, &old_sha, &new_sha) {
            Ok(true) => {} // fast-forward: fine
            Ok(false) => {
                eprintln!(
                    "remote: REJECTED {ref_name} ({old_sha} -> {new_sha}): \
                     force-pushes are blocked on protected branch '{branch}'"
                );
                rejected = true;
            }
            Err(e) => {
                eprintln!(
                    "remote: warning: could not determine fast-forward status for {branch}: {e}"
                );
            }
        }
    }

    Ok(if rejected { 1 } else { 0 })
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

/// Background task: every 60s, fetch updates for any repo mirror whose sync
/// interval has elapsed, via `git fetch --prune <remote_url>` run in the
/// repository's bare directory.
async fn sync_repo_mirrors(app_ctx: AppContext) {
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
    loop {
        interval.tick().await;

        let now = chrono::Utc::now();
        let mirrors = match entity::prelude::RepoMirror::find().all(&app_ctx.db).await {
            Ok(m) => m,
            Err(e) => {
                tracing::warn!("mirror sync: failed to list repo mirrors: {e}");
                continue;
            }
        };

        for mirror in mirrors {
            let due = match mirror.last_synced_at {
                None => true,
                Some(last_synced_at) => {
                    last_synced_at
                        + chrono::Duration::minutes(mirror.sync_interval_minutes as i64)
                        < now
                }
            };
            if !due {
                continue;
            }

            let repo = match entity::prelude::Repository::find_by_id(mirror.repo_id)
                .one(&app_ctx.db)
                .await
            {
                Ok(Some(r)) => r,
                Ok(None) => continue,
                Err(e) => {
                    tracing::warn!("mirror sync: failed to load repository {}: {e}", mirror.repo_id);
                    continue;
                }
            };

            let owner_login = if repo.owner_type == "organization" {
                entity::prelude::Organization::find_by_id(repo.owner_id)
                    .one(&app_ctx.db)
                    .await
                    .ok()
                    .flatten()
                    .map(|o| o.name)
            } else {
                entity::prelude::User::find_by_id(repo.owner_id)
                    .one(&app_ctx.db)
                    .await
                    .ok()
                    .flatten()
                    .map(|u| u.username)
            };
            let Some(owner_login) = owner_login else {
                continue;
            };

            let repo_path = match app_ctx.repo_manager.repo_path(&owner_login, &repo.name) {
                Ok(p) => p,
                Err(e) => {
                    tracing::warn!("mirror sync: failed to resolve repo path for {}/{}: {e}", owner_login, repo.name);
                    continue;
                }
            };

            let result = tokio::process::Command::new("git")
                .arg("fetch")
                .arg("--prune")
                .arg(&mirror.remote_url)
                .current_dir(&repo_path)
                .output()
                .await;

            match result {
                Ok(output) if output.status.success() => {
                    let mut active: entity::repo_mirror::ActiveModel = mirror.into();
                    active.last_synced_at = Set(Some(now));
                    if let Err(e) = active.update(&app_ctx.db).await {
                        tracing::warn!("mirror sync: failed to update last_synced_at: {e}");
                    }
                }
                Ok(output) => {
                    tracing::warn!(
                        "mirror sync: git fetch failed for {}/{}: {}",
                        owner_login,
                        repo.name,
                        String::from_utf8_lossy(&output.stderr)
                    );
                }
                Err(e) => {
                    tracing::warn!("mirror sync: failed to spawn git fetch for {}/{}: {e}", owner_login, repo.name);
                }
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

// ---------------------------------------------------------------------
// Artifact downloads
// ---------------------------------------------------------------------

/// Streams a previously-collected workflow artifact tarball from disk,
/// after checking the requesting user has at least read access to the
/// repository the artifact's workflow run belongs to.
async fn download_artifact_handler(
    State(state): State<ServerState>,
    AxumPath(id): AxumPath<String>,
    headers: HeaderMap,
) -> ServerResult<Response> {
    let artifact_id = Uuid::parse_str(&id)
        .map_err(|_| ServerError::BadRequest("invalid artifact id".to_string()))?;

    let artifact = entity::prelude::WorkflowArtifact::find_by_id(artifact_id)
        .one(&state.app_ctx.db)
        .await?
        .ok_or_else(|| ServerError::NotFound(format!("artifact {id} not found")))?;

    let run = entity::prelude::WorkflowRun::find_by_id(artifact.run_id)
        .one(&state.app_ctx.db)
        .await?
        .ok_or_else(|| ServerError::NotFound("workflow run not found".to_string()))?;

    let repo = entity::prelude::Repository::find_by_id(run.repo_id)
        .one(&state.app_ctx.db)
        .await?
        .ok_or_else(|| ServerError::NotFound("repository not found".to_string()))?;

    let user = auth::extract_user_from_headers(&headers, &state.app_ctx.jwt_secret, None).await;
    let user_id = user.as_ref().map(|c| c.sub);
    let allowed = user_has_repo_read_access(&state.app_ctx.db, &repo, user_id).await?;
    if !allowed {
        return Err(ServerError::Unauthorized);
    }

    let data = tokio::fs::read(&artifact.file_path).await.map_err(|e| {
        ServerError::Internal(anyhow::anyhow!(
            "failed to read artifact file {}: {e}",
            artifact.file_path
        ))
    })?;

    let content_disposition = format!("attachment; filename=\"{}.tar\"", artifact.name);
    Ok((
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, "application/x-tar".to_string()),
            (header::CONTENT_DISPOSITION, content_disposition),
        ],
        data,
    )
        .into_response())
}

/// Mirrors `graphql_api::mutation::repo_permission`'s effective-permission
/// computation (owner / org admin-or-owner / collaborator row / public
/// visibility), collapsed to a plain read-access boolean since this route
/// doesn't need the finer-grained `Permission` levels.
async fn user_has_repo_read_access(
    db: &DatabaseConnection,
    repo: &entity::repository::Model,
    user_id: Option<Uuid>,
) -> ServerResult<bool> {
    if !repo.is_private {
        return Ok(true);
    }
    let Some(user_id) = user_id else {
        return Ok(false);
    };

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
            entity::repo_collaborator::permission::ADMIN => auth::Permission::Admin,
            entity::repo_collaborator::permission::WRITE => auth::Permission::Write,
            _ => auth::Permission::Read,
        });

    Ok(
        auth::effective_permission(is_owner, is_admin_org_role, collaborator_perm, repo.is_private)
            .is_some(),
    )
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

// ---------------------------------------------------------------------
// OAuth2 provider
// ---------------------------------------------------------------------
//
// Genome acting AS an OAuth2 identity provider for third-party apps (the
// reverse of "login via Google"): third-party apps register an
// `oauth2_applications` row (via the `createOAuth2Application` GraphQL
// mutation) and then redirect users through `/oauth/authorize` to obtain a
// short-lived authorization code, which they exchange at `/oauth/token` for
// a bearer access token.
//
// NOTE (scope cut): the issued OAuth2 access tokens are not yet wired into
// `extract_user_from_headers`/`TokenLookup`, so they cannot (currently)
// authenticate GraphQL requests the way personal access tokens do. That
// integration was explicitly deprioritized per the task's cut list in favor
// of getting both HTTP routes below fully working first.

/// Percent-decodes a `application/x-www-form-urlencoded` component
/// (`+` -> space, `%XX` -> byte). Minimal, dependency-free implementation
/// sufficient for the plain ASCII client_id/secret/code/redirect_uri values
/// exchanged by this simplified OAuth2 flow.
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' if i + 2 < bytes.len() => {
                if let Ok(byte) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                    out.push(byte);
                    i += 3;
                } else {
                    out.push(bytes[i]);
                    i += 1;
                }
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Percent-encodes a string for safe embedding as a single query-string
/// value (e.g. the `then=` redirect target).
fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Parses an `application/x-www-form-urlencoded` body into a key/value map.
fn parse_form_body(body: &[u8]) -> HashMap<String, String> {
    let text = String::from_utf8_lossy(body);
    let mut map = HashMap::new();
    for pair in text.split('&') {
        if pair.is_empty() {
            continue;
        }
        let mut parts = pair.splitn(2, '=');
        let key = parts.next().unwrap_or_default();
        let value = parts.next().unwrap_or_default();
        map.insert(percent_decode(key), percent_decode(value));
    }
    map
}

/// `GET /oauth/authorize?client_id=&redirect_uri=&response_type=code&scope=`
///
/// Requires the requesting user to already be authenticated (via JWT bearer
/// token or PAT, same as GraphQL). If not authenticated, redirects (302) to
/// the frontend's `/login?then=<url-encoded original request url>` so the
/// user can log in and be sent back here. If authenticated: validates
/// `client_id` exists and that `redirect_uri` matches the one stored for the
/// application, generates a short-lived (10 minute) authorization code, and
/// redirects (302) to `{redirect_uri}?code={code}`.
async fn oauth_authorize_handler(
    State(state): State<ServerState>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
) -> ServerResult<Response> {
    let client_id = params
        .get("client_id")
        .cloned()
        .ok_or_else(|| ServerError::BadRequest("missing client_id".to_string()))?;
    let redirect_uri = params
        .get("redirect_uri")
        .cloned()
        .ok_or_else(|| ServerError::BadRequest("missing redirect_uri".to_string()))?;

    let user = auth::extract_user_from_headers(&headers, &state.app_ctx.jwt_secret, None).await;

    let Some(claims) = user else {
        let query = params
            .iter()
            .map(|(k, v)| format!("{}={}", percent_encode(k), percent_encode(v)))
            .collect::<Vec<_>>()
            .join("&");
        let original_url = format!("/oauth/authorize?{query}");
        let login_url = format!(
            "{}/login?then={}",
            state.config.frontend_url.trim_end_matches('/'),
            percent_encode(&original_url)
        );
        return Ok(Redirect::to(&login_url).into_response());
    };

    let application = entity::prelude::Oauth2Application::find()
        .filter(entity::oauth2_application::Column::ClientId.eq(client_id))
        .one(&state.app_ctx.db)
        .await?
        .ok_or_else(|| ServerError::BadRequest("unknown client_id".to_string()))?;

    if application.redirect_uri != redirect_uri {
        return Err(ServerError::BadRequest(
            "redirect_uri does not match the application's registered redirect_uri".to_string(),
        ));
    }

    let (code, _) = auth::generate_access_token();
    let code_row = entity::oauth2_authorization_code::ActiveModel {
        code: Set(code.clone()),
        application_id: Set(application.id),
        user_id: Set(claims.sub),
        redirect_uri: Set(redirect_uri.clone()),
        expires_at: Set(chrono::Utc::now() + chrono::Duration::minutes(10)),
        used: Set(false),
    };
    code_row.insert(&state.app_ctx.db).await?;

    let redirect_to = format!("{redirect_uri}?code={code}");
    Ok(Redirect::to(&redirect_to).into_response())
}

/// `POST /oauth/token` — form (`application/x-www-form-urlencoded`) or JSON
/// body with `client_id`, `client_secret`, `code`, `redirect_uri`,
/// `grant_type`. Validates the client credentials against the stored hash,
/// validates the authorization code (exists, not expired, not used, matches
/// the application and redirect_uri), marks it used, then issues a 1-hour
/// bearer access token and returns
/// `{access_token, token_type: "bearer", expires_in}`.
async fn oauth_token_handler(
    State(state): State<ServerState>,
    headers: HeaderMap,
    body: Bytes,
) -> ServerResult<Response> {
    let content_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    let params: HashMap<String, String> = if content_type.contains("application/json") {
        serde_json::from_slice(&body)
            .map_err(|e| ServerError::BadRequest(format!("invalid JSON body: {e}")))?
    } else {
        parse_form_body(&body)
    };

    let get_param = |key: &str| -> ServerResult<String> {
        params
            .get(key)
            .cloned()
            .ok_or_else(|| ServerError::BadRequest(format!("missing {key}")))
    };

    let client_id = get_param("client_id")?;
    let client_secret = get_param("client_secret")?;
    let code = get_param("code")?;
    let redirect_uri = get_param("redirect_uri")?;
    let _grant_type = params.get("grant_type").cloned().unwrap_or_default();

    let application = entity::prelude::Oauth2Application::find()
        .filter(entity::oauth2_application::Column::ClientId.eq(client_id))
        .one(&state.app_ctx.db)
        .await?
        .ok_or_else(|| ServerError::BadRequest("invalid client_id or client_secret".to_string()))?;

    if auth::hash_token(&client_secret) != application.client_secret_hash {
        return Err(ServerError::BadRequest(
            "invalid client_id or client_secret".to_string(),
        ));
    }

    let code_row = entity::prelude::Oauth2AuthorizationCode::find_by_id(code)
        .one(&state.app_ctx.db)
        .await?
        .ok_or_else(|| ServerError::BadRequest("invalid or expired authorization code".to_string()))?;

    if code_row.used
        || code_row.expires_at < chrono::Utc::now()
        || code_row.application_id != application.id
        || code_row.redirect_uri != redirect_uri
    {
        return Err(ServerError::BadRequest(
            "invalid or expired authorization code".to_string(),
        ));
    }

    let user_id = code_row.user_id;
    let mut used_code: entity::oauth2_authorization_code::ActiveModel = code_row.into();
    used_code.used = Set(true);
    used_code.update(&state.app_ctx.db).await?;

    let (access_token, access_token_hash) = auth::generate_access_token();
    const EXPIRES_IN_SECONDS: i64 = 3600;
    let token_row = entity::oauth2_access_token::ActiveModel {
        token_hash: Set(access_token_hash),
        application_id: Set(application.id),
        user_id: Set(user_id),
        scopes: Set(String::new()),
        expires_at: Set(chrono::Utc::now() + chrono::Duration::seconds(EXPIRES_IN_SECONDS)),
    };
    token_row.insert(&state.app_ctx.db).await?;

    Ok(axum::Json(serde_json::json!({
        "access_token": access_token,
        "token_type": "bearer",
        "expires_in": EXPIRES_IN_SECONDS,
    }))
    .into_response())
}

// ---------------------------------------------------------------------
// Package registry
// ---------------------------------------------------------------------

/// `PUT /packages/:owner/:name/:version` — authenticated upload of a raw
/// package file. Requires the authenticated user's username to match
/// `:owner` (no org support, per the simplified scope of this feature).
/// Packages are immutable once published: if a row already exists for
/// (owner, name, version) this returns 409 Conflict rather than overwriting.
async fn package_upload_handler(
    State(state): State<ServerState>,
    headers: HeaderMap,
    AxumPath((owner, name, version)): AxumPath<(String, String, String)>,
    body: Bytes,
) -> ServerResult<Response> {
    let claims = auth::extract_user_from_headers(&headers, &state.app_ctx.jwt_secret, None)
        .await
        .ok_or(ServerError::Unauthorized)?;
    if claims.username != owner {
        return Err(ServerError::Unauthorized);
    }

    let owner_user = entity::prelude::User::find()
        .filter(entity::user::Column::Username.eq(owner.clone()))
        .one(&state.app_ctx.db)
        .await?
        .ok_or_else(|| ServerError::NotFound(format!("user {owner} not found")))?;

    let existing = entity::prelude::Package::find()
        .filter(entity::package::Column::OwnerId.eq(owner_user.id))
        .filter(entity::package::Column::Name.eq(name.clone()))
        .filter(entity::package::Column::Version.eq(version.clone()))
        .one(&state.app_ctx.db)
        .await?;
    if existing.is_some() {
        return Err(ServerError::Conflict(format!(
            "package {owner}/{name}@{version} has already been published and cannot be overwritten"
        )));
    }

    let dir = std::path::Path::new(&state.config.packages_root_path)
        .join(&owner)
        .join(&name)
        .join(&version);
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|e| ServerError::Internal(e.into()))?;

    let file_name = format!("{name}-{version}.pkg");
    let file_path = dir.join(&file_name);
    tokio::fs::write(&file_path, &body)
        .await
        .map_err(|e| ServerError::Internal(e.into()))?;

    let package = entity::package::ActiveModel {
        id: Set(Uuid::new_v4()),
        repo_id: Set(None),
        owner_id: Set(owner_user.id),
        name: Set(name),
        version: Set(version),
        package_type: Set("generic".to_string()),
        file_path: Set(file_path.to_string_lossy().to_string()),
        size_bytes: Set(body.len() as i64),
        created_at: Set(chrono::Utc::now()),
    };
    package.insert(&state.app_ctx.db).await?;

    Ok(StatusCode::CREATED.into_response())
}

/// `GET /packages/:owner/:name/:version` — downloads a previously published
/// package's raw file content as `application/octet-stream`. Public: no
/// auth required, matching Forgejo's default generic-package read behavior.
async fn package_download_handler(
    State(state): State<ServerState>,
    AxumPath((owner, name, version)): AxumPath<(String, String, String)>,
) -> ServerResult<Response> {
    let owner_user = entity::prelude::User::find()
        .filter(entity::user::Column::Username.eq(owner.clone()))
        .one(&state.app_ctx.db)
        .await?
        .ok_or_else(|| ServerError::NotFound(format!("user {owner} not found")))?;

    let package = entity::prelude::Package::find()
        .filter(entity::package::Column::OwnerId.eq(owner_user.id))
        .filter(entity::package::Column::Name.eq(name.clone()))
        .filter(entity::package::Column::Version.eq(version.clone()))
        .one(&state.app_ctx.db)
        .await?
        .ok_or_else(|| ServerError::NotFound(format!("package {owner}/{name}@{version} not found")))?;

    let bytes = tokio::fs::read(&package.file_path)
        .await
        .map_err(|e| ServerError::Internal(e.into()))?;

    Ok((
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/octet-stream")],
        bytes,
    )
        .into_response())
}

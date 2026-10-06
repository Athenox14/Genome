mod config;
mod error;

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::{Path as AxumPath, Query, Request, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post, put};
use axum::Router;
use hiqlite::params;
use tower_governor::{governor::GovernorConfigBuilder, key_extractor::SmartIpKeyExtractor, GovernorLayer};
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

/// Resolves a hashed personal access token (`access_tokens` table) into the
/// `Claims` of the user it belongs to, for `auth::extract_user_from_headers`.
/// Expired tokens (`expires_at` in the past) are treated as invalid.
struct DbTokenLookup {
    db: hiqlite::Client,
}

#[async_trait::async_trait]
impl auth::TokenLookup for DbTokenLookup {
    async fn lookup(&self, token_hash: &str) -> Option<auth::Claims> {
        if let Some(token) = self
            .db
            .query_as::<entity::access_token::Model, _>(
                "SELECT * FROM access_tokens WHERE token_hash = ?1",
                params!(token_hash.to_string()),
            )
            .await
            .ok()
            .and_then(|rows| rows.into_iter().next())
        {
            if let Some(expires_at) = token.expires_at {
                if expires_at < chrono::Utc::now() {
                    return None;
                }
            }
            return self.claims_for_user(token.user_id).await;
        }

        None
    }
}

impl DbTokenLookup {
    async fn claims_for_user(&self, user_id: Uuid) -> Option<auth::Claims> {
        let user = self
            .db
            .query_as::<entity::user::Model, _>(
                "SELECT * FROM users WHERE id = ?1",
                params!(user_id.to_string()),
            )
            .await
            .ok()?
            .into_iter()
            .next()?;
        if user.deactivated_at.is_some() {
            return None;
        }
        Some(auth::Claims {
            sub: user.id,
            username: user.username,
            is_admin: user.is_admin,
            exp: usize::MAX,
            iat: 0,
        })
    }
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

    tokio::fs::create_dir_all(&config.data_dir).await?;

    let secret_raft = hiqlite_secret(&config.jwt_secret, "raft");
    let secret_api = hiqlite_secret(&config.jwt_secret, "api");
    // Hiqlite always encrypts its Raft logs/snapshots at rest and requires a
    // non-empty `enc_keys`, regardless of whether the `backup`/`s3`/`dashboard`
    // features are enabled. Derive a stable 32-byte key from JWT_SECRET (same
    // seed as secret_raft/secret_api) so it survives restarts -- a fresh
    // random key every boot would make prior snapshots undecryptable.
    let enc_key_bytes: [u8; 32] = {
        use sha2::{Digest, Sha256};
        Sha256::digest(format!("{}:hiqlite-enc-key", config.jwt_secret).as_bytes()).into()
    };
    let node_config = hiqlite::NodeConfig {
        node_id: 1,
        nodes: vec![hiqlite::Node {
            id: 1,
            addr_raft: config.hiqlite_raft_addr.clone(),
            addr_api: config.hiqlite_api_addr.clone(),
        }],
        // `listen_addr_*` is just the bind HOST (no port) -- hiqlite derives
        // the port to bind from the matching `Node.addr_*` entry above and
        // appends it internally. Passing "host:port" here (as opposed to
        // just "host") produces an invalid doubled "host:port:port" address.
        listen_addr_api: "0.0.0.0".into(),
        listen_addr_raft: "0.0.0.0".into(),
        data_dir: config.data_dir.clone().into(),
        filename_db: "genome.db".into(),
        secret_raft,
        secret_api,
        enc_keys: cryptr::EncKeys {
            enc_key_active: "k1".to_string(),
            enc_keys: vec![("k1".to_string(), enc_key_bytes.to_vec())],
        },
        ..Default::default()
    };
    let db: hiqlite::Client = hiqlite::start_node(node_config).await?;
    db.migrate::<migration::Migrations>().await?;

    if let Some(bootstrap_token) = &config.admin_bootstrap_token {
        bootstrap_admin_token(&db, bootstrap_token).await?;
    }

    let repo_manager = Arc::new(git_core::RepoManager::new(config.repos_root_path.clone()));
    let artifacts_root = std::path::Path::new(&config.repos_root_path)
        .parent()
        .map(|p| p.join("artifacts"))
        .unwrap_or_else(|| std::path::PathBuf::from("./artifacts"));
    let actions_executor = Arc::new(actions::Executor::new_with_socket(
        artifacts_root,
        &config.docker_socket_path,
    )?);
    let workspace_manager = Arc::new(dev_env::WorkspaceManager::connect_socket(&config.docker_socket_path)?);
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
    // Per-IP token-bucket rate limiting via `tower_governor`: replenishes one
    // request allowance every 500ms (2/sec sustained, matching the previous
    // fixed-window limiter's ~120 req/min average) with a burst allowance of
    // 120 requests. Excess requests get a 429.
    //
    // Keyed by the CLIENT IP (`X-Forwarded-For` / `X-Real-IP` / `Forwarded`, falling back to the peer IP), not by
    // the peer IP alone: behind the ingress every request has the ingress pod as its peer, so all clients (users,
    // security scanner, uptime probes) shared ONE bucket and a scan starved the health check (429 on /health).
    let governor_conf = Arc::new(
        GovernorConfigBuilder::default()
            .key_extractor(SmartIpKeyExtractor)
            .per_millisecond(500)
            .burst_size(120)
            .finish()
            .expect("valid governor rate-limit configuration"),
    );

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
            "/workspaces/:id/proxy_port/:port_name/*path",
            get(workspace_proxy_named_handler).post(workspace_proxy_named_handler),
        )
        .route(
            "/packages/:owner/:name/:version",
            put(package_upload_handler).get(package_download_handler),
        )
        .route(
            "/artifacts/:id/download",
            get(download_artifact_handler),
        )
        .route("/runner/claim", post(runner_claim_handler))
        .route(
            "/runner/jobs/:id/complete",
            post(runner_complete_handler),
        )
        .layer(GovernorLayer {
            config: governor_conf,
        })
        // Added AFTER the rate-limit layer so health probes are never throttled (a layer only wraps the routes
        // declared before it). Trace and CORS below still apply to it.
        .route("/health", get(health_handler))
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
        auth::extract_user_from_headers(&headers, &state.app_ctx.jwt_secret, Some(&DbTokenLookup { db: state.app_ctx.db.clone() })).await;
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
            if let Err(e) =
                index_repo_code_on_push(app_ctx, owner_clone, repo_clone, changes).await
            {
                tracing::warn!("post-push code-search indexing failed: {e}");
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

/// Fire-and-forget task run after a successful `git-receive-pack`:
/// re-indexes the repository's default branch tip into `code_search_fts`
/// (SQLite FTS5) for the `search` GraphQL query's code-search results.
/// A no-op if the push didn't move the default branch. Re-indexing always
/// replaces the repo's whole previous index rather than diffing, since a
/// force-push or history rewrite can change any file, not just the ones
/// in the immediate diff.
async fn index_repo_code_on_push(
    app_ctx: AppContext,
    owner: String,
    repo: String,
    changes: Vec<(String, String, String)>,
) -> anyhow::Result<()> {
    const ZERO_SHA: &str = "0000000000000000000000000000000000000000";
    // Indexing-cost guards (see `RepoManager::list_text_blobs_at_ref`), not
    // correctness requirements: a huge/binary-heavy repo gets a partial
    // index rather than an expensive or garbage one.
    const MAX_FILES: usize = 2000;
    const MAX_FILE_BYTES: usize = 256 * 1024;

    let repo_row = app_ctx
        .db
        .query_as::<entity::repository::Model, _>(
            "SELECT * FROM repositories WHERE name = ?1",
            params!(repo.clone()),
        )
        .await?
        .into_iter()
        .next();
    let Some(repo_row) = repo_row else {
        return Ok(());
    };

    let default_ref = format!("refs/heads/{}", repo_row.default_branch);
    let Some((_, _, new_sha)) = changes.iter().find(|(r, _, _)| *r == default_ref) else {
        return Ok(());
    };
    if new_sha == ZERO_SHA {
        // Default branch was deleted outright: drop its index entirely.
        app_ctx
            .db
            .execute(
                "DELETE FROM code_search_fts WHERE repo_id = ?1",
                params!(repo_row.id.to_string()),
            )
            .await?;
        return Ok(());
    }

    let blobs =
        app_ctx
            .repo_manager
            .list_text_blobs_at_ref(&owner, &repo, new_sha, MAX_FILES, MAX_FILE_BYTES)?;

    app_ctx
        .db
        .execute(
            "DELETE FROM code_search_fts WHERE repo_id = ?1",
            params!(repo_row.id.to_string()),
        )
        .await?;

    for (path, content) in blobs {
        let text = String::from_utf8_lossy(&content).into_owned();
        app_ctx
            .db
            .execute(
                "INSERT INTO code_search_fts (repo_id, path, content) VALUES (?1, ?2, ?3)",
                params!(repo_row.id.to_string(), path, text),
            )
            .await?;
    }

    Ok(())
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

    let repo_row = app_ctx
        .db
        .query_as::<entity::repository::Model, _>(
            "SELECT * FROM repositories WHERE name = ?1",
            params!(repo.clone()),
        )
        .await?
        .into_iter()
        .next();

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
            let res = app_ctx
                .db
                .execute(
                    "INSERT INTO activity_events (id, repo_id, actor_id, kind, summary, created_at) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    params!(
                        Uuid::new_v4().to_string(),
                        Some(repo_row.id.to_string()),
                        repo_row.owner_id.to_string(),
                        entity::activity_event::kind::PUSH.to_string(),
                        format!("push to {ref_name} on {owner}/{repo}"),
                        chrono::Utc::now().to_rfc3339()
                    ),
                )
                .await;
            if let Err(e) = res {
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

            let run_id = Uuid::new_v4();
            let started_at = chrono::Utc::now();
            let run_model = entity::workflow_run::Model {
                id: run_id,
                repo_id: repo_row.id,
                workflow_name: workflow_name.clone(),
                commit_sha: new_sha.clone(),
                event: "push".to_string(),
                status: entity::workflow_run::status::RUNNING.to_string(),
                started_at: Some(started_at),
                finished_at: None,
            };
            let inserted = app_ctx
                .db
                .execute(
                    "INSERT INTO workflow_runs (id, repo_id, workflow_name, commit_sha, event, status, started_at, finished_at) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, NULL)",
                    params!(
                        run_id.to_string(),
                        repo_row.id.to_string(),
                        workflow_name.clone(),
                        new_sha.clone(),
                        "push".to_string(),
                        entity::workflow_run::status::RUNNING.to_string(),
                        started_at.to_rfc3339()
                    ),
                )
                .await;
            if let Err(e) = inserted {
                tracing::warn!("failed to record workflow_run: {e}");
                continue;
            }

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

                // Record this job in `runner_jobs` for history/observability,
                // but with status `HANDLED_INPROCESS` (not `QUEUED`) since
                // it's about to run right here -- `/runner/claim` only ever
                // selects `QUEUED` rows, so a standalone runner can never
                // pick this one up and double-execute it.
                let runner_job_payload = serde_json::json!({
                    "run_id": run_model.id,
                    "job": job,
                    "repo_archive_b64": base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &archive),
                    "env_extra": env_extra,
                    "secrets": secrets,
                })
                .to_string();
                let runner_job_res = app_ctx
                    .db
                    .execute(
                        "INSERT INTO runner_jobs (id, kind, repo_id, workflow_run_id, payload, status, claimed_by, claimed_at, created_at, finished_at) \
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL, NULL, ?7, NULL)",
                        params!(
                            Uuid::new_v4().to_string(),
                            entity::runner_job::kind::CI_JOB.to_string(),
                            Some(repo_row.id.to_string()),
                            Some(run_model.id.to_string()),
                            runner_job_payload,
                            entity::runner_job::status::HANDLED_INPROCESS.to_string(),
                            chrono::Utc::now().to_rfc3339()
                        ),
                    )
                    .await;
                if let Err(e) = runner_job_res {
                    tracing::warn!("failed to record runner_job history row (non-fatal): {e}");
                }

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
                            let res = app_ctx
                                .db
                                .execute(
                                    "INSERT INTO workflow_artifacts (id, run_id, job_id, name, file_path, size_bytes, created_at) \
                                     VALUES (?1, ?2, NULL, ?3, ?4, ?5, ?6)",
                                    params!(
                                        Uuid::new_v4().to_string(),
                                        run_model.id.to_string(),
                                        artifact.name.clone(),
                                        artifact.file_path.clone(),
                                        artifact.size_bytes,
                                        chrono::Utc::now().to_rfc3339()
                                    ),
                                )
                                .await;
                            if let Err(e) = res {
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

                let res = app_ctx
                    .db
                    .execute(
                        "UPDATE workflow_runs SET status = ?1, finished_at = ?2 WHERE id = ?3",
                        params!(
                            status.to_string(),
                            chrono::Utc::now().to_rfc3339(),
                            run_model.id.to_string()
                        ),
                    )
                    .await;
                if let Err(e) = res {
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

    // The server binary (running as its own process, embedding the actual
    // Hiqlite Raft node) is expected to already be running when a push
    // happens; this CLI subcommand connects to it as a remote Hiqlite client
    // rather than starting a second Raft node against the same data
    // directory (which would conflict with the running node's port bindings
    // and on-disk state).
    let jwt_secret = std::env::var("JWT_SECRET")
        .map_err(|_| anyhow::anyhow!("JWT_SECRET environment variable must be set"))?;
    let api_addr =
        std::env::var("HIQLITE_API_ADDR").unwrap_or_else(|_| "127.0.0.1:8200".to_string());
    let secret_api = hiqlite_secret(&jwt_secret, "api");
    let db = hiqlite::Client::remote(vec![api_addr], false, false, secret_api, false, None, None)
        .await?;

    // `Client::remote(...)` (used above, since this CLI subcommand connects
    // to the already-running server rather than starting its own Raft node)
    // only supports `query_map` (`T: From<&mut Row>`), not `query_as`
    // (`T: DeserializeOwned`) -- a real bug caught via dogfooding (pushing
    // Genome's own repo to a running Genome instance with a branch
    // protection rule set), see the `From<&mut Row>` impls on
    // `entity::repository::Model`/`entity::branch_protection_rule::Model`.
    let repo_row = db
        .query_map::<entity::repository::Model, _>(
            "SELECT * FROM repositories WHERE name = ?1",
            params!(name.clone()),
        )
        .await?
        .into_iter()
        .next();

    let Some(repo_row) = repo_row else {
        // No matching repository row (shouldn't normally happen for a repo
        // that has a hook at all); fail open rather than blocking pushes.
        return Ok(0);
    };

    let rules = db
        .query_map::<entity::branch_protection_rule::Model, _>(
            "SELECT * FROM branch_protection_rules WHERE repo_id = ?1",
            params!(repo_row.id.to_string()),
        )
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

// ---------------------------------------------------------------------
// Standalone runner (crates/runner) polling routes
// ---------------------------------------------------------------------

/// `POST /runner/claim` -- called by a standalone `runner` binary. Auth uses
/// the same bearer-token/PAT validation as every other authenticated route
/// (`auth::extract_user_from_headers` + `DbTokenLookup` against
/// `access_tokens`); any valid, non-expired token may claim jobs.
///
/// Hiqlite (rusqlite-based) support for `UPDATE ... RETURNING` was not
/// confirmed available in this codebase (no existing call site uses it), so
/// this claims atomically-enough for a single-node embedded Raft/SQLite
/// setup via an UPDATE tagging the oldest queued row with a unique
/// `claimed_by` token, followed by a SELECT for that same tag, rather than
/// relying on RETURNING.
async fn runner_claim_handler(
    State(state): State<ServerState>,
    headers: HeaderMap,
) -> ServerResult<Response> {
    let claims = auth::extract_user_from_headers(
        &headers,
        &state.app_ctx.jwt_secret,
        Some(&DbTokenLookup {
            db: state.app_ctx.db.clone(),
        }),
    )
    .await
    .ok_or(ServerError::Unauthorized)?;
    let _ = claims;

    let claim_tag = Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();

    let updated = state
        .app_ctx
        .db
        .execute(
            "UPDATE runner_jobs SET status = ?1, claimed_by = ?2, claimed_at = ?3 \
             WHERE id = (SELECT id FROM runner_jobs WHERE status = ?4 ORDER BY created_at ASC LIMIT 1)",
            params!(
                entity::runner_job::status::CLAIMED.to_string(),
                claim_tag.clone(),
                now,
                entity::runner_job::status::QUEUED.to_string()
            ),
        )
        .await?;

    if updated == 0 {
        return Ok(StatusCode::NO_CONTENT.into_response());
    }

    let claimed = state
        .app_ctx
        .db
        .query_as::<entity::runner_job::Model, _>(
            "SELECT * FROM runner_jobs WHERE claimed_by = ?1 AND status = ?2 ORDER BY claimed_at DESC LIMIT 1",
            params!(claim_tag, entity::runner_job::status::CLAIMED.to_string()),
        )
        .await?
        .into_iter()
        .next();

    let Some(job) = claimed else {
        return Ok(StatusCode::NO_CONTENT.into_response());
    };

    Ok(axum::Json(serde_json::json!({
        "id": job.id,
        "kind": job.kind,
        "payload": job.payload,
    }))
    .into_response())
}

#[derive(serde::Deserialize)]
struct RunnerCompleteRequest {
    status: String,
    #[allow(dead_code)]
    logs: String,
    /// Structured result JSON, currently only produced for
    /// `dev_workspace_action` jobs (e.g. `{"container_id": ..., "runner_id": ...}`
    /// for `create`, `{"output": ...}` for `exec`) -- `None` for CI jobs.
    /// Stored verbatim on `runner_jobs.result` for a polling GraphQL
    /// mutation (see `wait_for_runner_job` in `graphql-api::mutation`) to
    /// read back.
    #[serde(default)]
    result: Option<String>,
}

/// `POST /runner/jobs/:id/complete` -- reports the result of a job claimed
/// via `/runner/claim`. Updates the `runner_jobs` row and, if the job is
/// tied to a `workflow_run` (`workflow_run_id` set), updates that row's
/// status too.
///
/// The status-update-on-completion logic here is a minimal, deliberately
/// duplicated variant of the equivalent block in `process_push_workflows`
/// above (same UPDATE against `workflow_runs`) -- see that function's
/// `DUAL-PATH` comment for context on why both an in-process path and this
/// queue-based path exist simultaneously. A shared helper was not factored
/// out to keep this change purely additive to the existing, working
/// in-process path.
async fn runner_complete_handler(
    State(state): State<ServerState>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<String>,
    axum::Json(body): axum::Json<RunnerCompleteRequest>,
) -> ServerResult<Response> {
    auth::extract_user_from_headers(
        &headers,
        &state.app_ctx.jwt_secret,
        Some(&DbTokenLookup {
            db: state.app_ctx.db.clone(),
        }),
    )
    .await
    .ok_or(ServerError::Unauthorized)?;

    let job_id = Uuid::parse_str(&id)
        .map_err(|_| ServerError::BadRequest("invalid job id".to_string()))?;

    let status = match body.status.as_str() {
        "success" => entity::runner_job::status::SUCCESS,
        _ => entity::runner_job::status::FAILURE,
    };
    let now = chrono::Utc::now().to_rfc3339();

    let job = state
        .app_ctx
        .db
        .query_as::<entity::runner_job::Model, _>(
            "SELECT * FROM runner_jobs WHERE id = ?1",
            params!(job_id.to_string()),
        )
        .await?
        .into_iter()
        .next()
        .ok_or_else(|| ServerError::NotFound(format!("runner job {id} not found")))?;

    state
        .app_ctx
        .db
        .execute(
            "UPDATE runner_jobs SET status = ?1, finished_at = ?2, result = ?3 WHERE id = ?4",
            params!(status.to_string(), now.clone(), body.result.clone(), job_id.to_string()),
        )
        .await?;

    if let Some(run_id) = job.workflow_run_id {
        // duplicated from `process_push_workflows`'s workflow_run status
        // update, see comment above on why this isn't factored into a
        // shared helper yet.
        let run_status = match status {
            entity::runner_job::status::SUCCESS => entity::workflow_run::status::SUCCESS,
            _ => entity::workflow_run::status::FAILURE,
        };
        let res = state
            .app_ctx
            .db
            .execute(
                "UPDATE workflow_runs SET status = ?1, finished_at = ?2 WHERE id = ?3",
                params!(run_status.to_string(), now, run_id.to_string()),
            )
            .await;
        if let Err(e) = res {
            tracing::warn!("failed to update workflow_run from runner completion: {e}");
        }
    }

    Ok(StatusCode::OK.into_response())
}

fn strip_git_suffix(name: &str) -> &str {
    name.strip_suffix(".git").unwrap_or(name)
}

/// Derives a Hiqlite Raft/API secret (must be at least 16 characters) from
/// `JWT_SECRET` plus a fixed per-purpose suffix, so the single embedded node
/// doesn't need its own separate secret env vars for a purely local,
/// loopback-only Raft "cluster" of one. Padded if the input is short.
fn hiqlite_secret(seed: &str, purpose: &str) -> String {
    let mut secret = format!("{seed}-hiqlite-{purpose}");
    while secret.len() < 16 {
        secret.push('0');
    }
    secret
}

/// Idempotently ensures an admin user + a personal access token hashing to
/// `bootstrap_token` both exist, so `Authorization: token <bootstrap_token>`
/// authenticates as an admin immediately on every startup with no
/// `register`/`login` step ever required -- the intended bootstrap path for
/// deployments that are pure API/automation with no interactive use.
async fn bootstrap_admin_token(db: &hiqlite::Client, bootstrap_token: &str) -> anyhow::Result<()> {
    let token_hash = auth::hash_token(bootstrap_token);

    let existing = db
        .query_as::<entity::access_token::Model, _>(
            "SELECT * FROM access_tokens WHERE token_hash = ?1",
            params!(token_hash.clone()),
        )
        .await?;
    if !existing.is_empty() {
        tracing::info!("admin bootstrap token already provisioned, skipping");
        return Ok(());
    }

    const BOOTSTRAP_USERNAME: &str = "admin";
    let admin_user = db
        .query_as::<entity::user::Model, _>(
            "SELECT * FROM users WHERE username = ?1",
            params!(BOOTSTRAP_USERNAME.to_string()),
        )
        .await?
        .into_iter()
        .next();

    let admin_user_id = match admin_user {
        Some(u) => u.id,
        None => {
            let id = Uuid::new_v4();
            // No one is ever meant to log in as this account with a
            // password -- it exists solely to own the bootstrap PAT -- so
            // its password hash is a random, never-recorded value.
            let random_unusable_password = Uuid::new_v4().to_string();
            let password_hash = auth::hash_password(&random_unusable_password)
                .map_err(|e| anyhow::anyhow!("failed to hash bootstrap admin password: {e}"))?;
            db.execute(
                "INSERT INTO users (id, username, email, password_hash, is_admin, avatar_url, created_at, deactivated_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, NULL, ?6, NULL)",
                params!(
                    id.to_string(),
                    BOOTSTRAP_USERNAME.to_string(),
                    "admin@localhost".to_string(),
                    password_hash,
                    1i64,
                    chrono::Utc::now().to_rfc3339()
                ),
            )
            .await?;
            tracing::info!("created bootstrap admin user '{BOOTSTRAP_USERNAME}'");
            id
        }
    };

    db.execute(
        "INSERT INTO access_tokens (id, user_id, token_hash, name, scopes, expires_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, NULL)",
        params!(
            Uuid::new_v4().to_string(),
            admin_user_id.to_string(),
            token_hash,
            "bootstrap-admin-token".to_string(),
            "[]".to_string()
        ),
    )
    .await?;
    tracing::info!("provisioned admin bootstrap access token");

    Ok(())
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

    let rows = app_ctx
        .db
        .query_as::<entity::repo_secret::Model, _>(
            "SELECT * FROM repo_secrets WHERE repo_id = ?1",
            params!(repo_id.to_string()),
        )
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

        let workspaces = match app_ctx
            .db
            .query_as::<entity::dev_workspace::Model, _>(
                "SELECT * FROM dev_workspaces WHERE status = ?1",
                params!(entity::dev_workspace::status::RUNNING.to_string()),
            )
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

            if workspace.runner_id.is_some() {
                // Auto-stop isn't wired up for runner-hosted workspaces yet
                // (see `stop_dev_workspace`'s same restriction) -- their
                // container lives on a different Docker daemon than the
                // one `app_ctx.workspace_manager` talks to.
                continue;
            }
            let Some(container_id) = workspace.container_id.clone() else {
                continue;
            };

            if let Err(e) = app_ctx.workspace_manager.stop_workspace(&container_id).await {
                tracing::warn!("auto-stop: failed to stop workspace {}: {e}", workspace.id);
                continue;
            }

            let res = app_ctx
                .db
                .execute(
                    "UPDATE dev_workspaces SET status = ?1 WHERE id = ?2",
                    params!(
                        entity::dev_workspace::status::STOPPED.to_string(),
                        workspace.id.to_string()
                    ),
                )
                .await;
            if let Err(e) = res {
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
        let mirrors = match app_ctx
            .db
            .query_as::<entity::repo_mirror::Model, _>("SELECT * FROM repo_mirrors", vec![])
            .await
        {
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

            let repo = match app_ctx
                .db
                .query_as::<entity::repository::Model, _>(
                    "SELECT * FROM repositories WHERE id = ?1",
                    params!(mirror.repo_id.to_string()),
                )
                .await
            {
                Ok(rows) => match rows.into_iter().next() {
                    Some(r) => r,
                    None => continue,
                },
                Err(e) => {
                    tracing::warn!("mirror sync: failed to load repository {}: {e}", mirror.repo_id);
                    continue;
                }
            };

            let owner_login = if repo.owner_type == "organization" {
                app_ctx
                    .db
                    .query_as::<entity::organization::Model, _>(
                        "SELECT * FROM organizations WHERE id = ?1",
                        params!(repo.owner_id.to_string()),
                    )
                    .await
                    .ok()
                    .and_then(|rows| rows.into_iter().next())
                    .map(|o| o.name)
            } else {
                app_ctx
                    .db
                    .query_as::<entity::user::Model, _>(
                        "SELECT * FROM users WHERE id = ?1",
                        params!(repo.owner_id.to_string()),
                    )
                    .await
                    .ok()
                    .and_then(|rows| rows.into_iter().next())
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

            // Refspecs explicites : sans elles, `git fetch <url>` dans un dépôt bare ne met à jour que
            // FETCH_HEAD, jamais les branches ni les tags — le « miroir » ne reflétait rien. `+` force la
            // mise à jour (réécritures d'historique en amont) et `--prune` retire ce qui a disparu en amont.
            let result = tokio::process::Command::new("git")
                .arg("fetch")
                .arg("--prune")
                .arg(&mirror.remote_url)
                .arg("+refs/heads/*:refs/heads/*")
                .arg("+refs/tags/*:refs/tags/*")
                .current_dir(&repo_path)
                .output()
                .await;

            match result {
                Ok(output) if output.status.success() => {
                    let res = app_ctx
                        .db
                        .execute(
                            "UPDATE repo_mirrors SET last_synced_at = ?1 WHERE id = ?2",
                            params!(now.to_rfc3339(), mirror.id.to_string()),
                        )
                        .await;
                    if let Err(e) = res {
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

/// `/workspaces/:id/proxy/*path` — proxies to the workspace's default port
/// (named "http" if bound, else whichever single port the workspace has).
/// Kept for backward compatibility with callers written before named ports.
async fn workspace_proxy_handler(
    State(state): State<ServerState>,
    AxumPath((id, path)): AxumPath<(String, String)>,
    req: Request,
) -> ServerResult<Response> {
    proxy_workspace_request(state, id, None, path, req).await
}

/// `/workspaces/:id/proxy_port/:port_name/*path` — proxies to an explicitly
/// named port on the workspace (e.g. a second service alongside code-server).
async fn workspace_proxy_named_handler(
    State(state): State<ServerState>,
    AxumPath((id, port_name, path)): AxumPath<(String, String, String)>,
    req: Request,
) -> ServerResult<Response> {
    proxy_workspace_request(state, id, Some(port_name), path, req).await
}

async fn proxy_workspace_request(
    state: ServerState,
    id: String,
    port_name: Option<String>,
    path: String,
    req: Request,
) -> ServerResult<Response> {
    let workspace_id = Uuid::parse_str(&id)
        .map_err(|_| ServerError::BadRequest("invalid workspace id".to_string()))?;

    let workspace = state
        .app_ctx
        .db
        .query_as::<entity::dev_workspace::Model, _>(
            "SELECT * FROM dev_workspaces WHERE id = ?1",
            params!(workspace_id.to_string()),
        )
        .await?
        .into_iter()
        .next()
        .ok_or_else(|| ServerError::NotFound(format!("workspace {id} not found")))?;

    if workspace.runner_id.is_some() {
        // Known gap (see `create_dev_workspace`'s `on_runner` doc comment):
        // a runner-hosted workspace's container lives on the runner's own
        // Docker daemon, which `server` has no network path to reach or
        // proxy through yet (no reverse tunnel exists between them).
        return Err(ServerError::BadRequest(
            "live port-proxying to runner-hosted dev workspaces is not supported yet".to_string(),
        ));
    }

    let container_id = workspace
        .container_id
        .ok_or_else(|| ServerError::BadRequest("workspace has no container".to_string()))?;

    let host_port = resolve_port(&state.app_ctx.workspace_manager, &container_id, port_name.as_deref()).await?;

    // Best-effort activity bump: never block the proxied request on this.
    {
        let db = state.app_ctx.db.clone();
        tokio::spawn(async move {
            let res = db
                .execute(
                    "UPDATE dev_workspaces SET last_activity_at = ?1 WHERE id = ?2",
                    params!(chrono::Utc::now().to_rfc3339(), workspace_id.to_string()),
                )
                .await;
            if let Err(e) = res {
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

    let artifact = state
        .app_ctx
        .db
        .query_as::<entity::workflow_artifact::Model, _>(
            "SELECT * FROM workflow_artifacts WHERE id = ?1",
            params!(artifact_id.to_string()),
        )
        .await?
        .into_iter()
        .next()
        .ok_or_else(|| ServerError::NotFound(format!("artifact {id} not found")))?;

    let run = state
        .app_ctx
        .db
        .query_as::<entity::workflow_run::Model, _>(
            "SELECT * FROM workflow_runs WHERE id = ?1",
            params!(artifact.run_id.to_string()),
        )
        .await?
        .into_iter()
        .next()
        .ok_or_else(|| ServerError::NotFound("workflow run not found".to_string()))?;

    let repo = state
        .app_ctx
        .db
        .query_as::<entity::repository::Model, _>(
            "SELECT * FROM repositories WHERE id = ?1",
            params!(run.repo_id.to_string()),
        )
        .await?
        .into_iter()
        .next()
        .ok_or_else(|| ServerError::NotFound("repository not found".to_string()))?;

    let user = auth::extract_user_from_headers(&headers, &state.app_ctx.jwt_secret, Some(&DbTokenLookup { db: state.app_ctx.db.clone() })).await;
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
    db: &hiqlite::Client,
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
            entity::repo_collaborator::permission::ADMIN => auth::Permission::Admin,
            entity::repo_collaborator::permission::WRITE => auth::Permission::Write,
            _ => auth::Permission::Read,
        });

    Ok(
        auth::effective_permission(is_owner, is_admin_org_role, collaborator_perm, repo.is_private)
            .is_some(),
    )
}

/// Resolves the host port for a workspace container, either for an
/// explicitly named port (`Some("http")`) or the workspace's default port
/// (`None`). Returns a clear 404 if the container or the named port isn't
/// found, rather than silently proxying to the wrong service.
async fn resolve_port(
    workspace_manager: &dev_env::WorkspaceManager,
    container_id: &str,
    port_name: Option<&str>,
) -> ServerResult<u16> {
    let handles = workspace_manager
        .list_workspaces("")
        .await
        .map_err(ServerError::DevEnv)?;
    let handle = handles
        .into_iter()
        .find(|h| h.container_id == container_id)
        .ok_or_else(|| ServerError::NotFound("workspace container not found".to_string()))?;

    match port_name {
        Some(name) => handle.port(name).ok_or_else(|| {
            ServerError::NotFound(format!("workspace has no port named '{name}'"))
        }),
        None => handle
            .default_port()
            .ok_or_else(|| ServerError::NotFound("workspace has no bound ports".to_string())),
    }
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
    let claims = auth::extract_user_from_headers(&headers, &state.app_ctx.jwt_secret, Some(&DbTokenLookup { db: state.app_ctx.db.clone() }))
        .await
        .ok_or(ServerError::Unauthorized)?;
    if claims.username != owner {
        return Err(ServerError::Unauthorized);
    }

    let owner_user = state
        .app_ctx
        .db
        .query_as::<entity::user::Model, _>(
            "SELECT * FROM users WHERE username = ?1",
            params!(owner.clone()),
        )
        .await?
        .into_iter()
        .next()
        .ok_or_else(|| ServerError::NotFound(format!("user {owner} not found")))?;

    let existing = state
        .app_ctx
        .db
        .query_as::<entity::package::Model, _>(
            "SELECT * FROM packages WHERE owner_id = ?1 AND name = ?2 AND version = ?3",
            params!(owner_user.id.to_string(), name.clone(), version.clone()),
        )
        .await?
        .into_iter()
        .next();
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

    state
        .app_ctx
        .db
        .execute(
            "INSERT INTO packages (id, repo_id, owner_id, name, version, package_type, file_path, size_bytes, created_at) \
             VALUES (?1, NULL, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params!(
                Uuid::new_v4().to_string(),
                owner_user.id.to_string(),
                name,
                version,
                "generic".to_string(),
                file_path.to_string_lossy().to_string(),
                body.len() as i64,
                chrono::Utc::now().to_rfc3339()
            ),
        )
        .await?;

    Ok(StatusCode::CREATED.into_response())
}

/// `GET /packages/:owner/:name/:version` — downloads a previously published
/// package's raw file content as `application/octet-stream`. Public: no
/// auth required, matching Forgejo's default generic-package read behavior.
async fn package_download_handler(
    State(state): State<ServerState>,
    AxumPath((owner, name, version)): AxumPath<(String, String, String)>,
) -> ServerResult<Response> {
    let owner_user = state
        .app_ctx
        .db
        .query_as::<entity::user::Model, _>(
            "SELECT * FROM users WHERE username = ?1",
            params!(owner.clone()),
        )
        .await?
        .into_iter()
        .next()
        .ok_or_else(|| ServerError::NotFound(format!("user {owner} not found")))?;

    let package = state
        .app_ctx
        .db
        .query_as::<entity::package::Model, _>(
            "SELECT * FROM packages WHERE owner_id = ?1 AND name = ?2 AND version = ?3",
            params!(owner_user.id.to_string(), name.clone(), version.clone()),
        )
        .await?
        .into_iter()
        .next()
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

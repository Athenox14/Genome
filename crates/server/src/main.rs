mod config;
mod error;

use std::collections::HashMap;
use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::{Path as AxumPath, Query, Request, State};
use axum::http::{header, HeaderMap, StatusCode};
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

    let app_ctx = AppContext {
        db: db.clone(),
        repo_manager: repo_manager.clone(),
        jwt_secret: config.jwt_secret.clone(),
        actions_executor: actions_executor.clone(),
        workspace_manager: workspace_manager.clone(),
    };

    let schema = graphql_api::build_schema(app_ctx.clone());

    let state = ServerState { schema, app_ctx };

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
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive())
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(&config.listen_addr).await?;
    tracing::info!("listening on {}", config.listen_addr);
    axum::serve(listener, app).await?;

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
        tokio::spawn(async move {
            if let Err(e) =
                process_push_workflows(app_ctx, owner_clone, repo_clone, changes).await
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

    for (ref_name, _old_sha, new_sha) in changes {
        if new_sha == ZERO_SHA {
            // Branch deletion: nothing to run.
            continue;
        }
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

            for (_job_id, job) in workflow.jobs.iter() {
                let env_extra = HashMap::new();
                let executor = app_ctx.actions_executor.clone();
                let job = job.clone();
                let archive: Vec<u8> = Vec::new(); // no repo archive materialization available here
                let result = executor
                    .run_job(&job, &archive, env_extra, |line| {
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

fn strip_git_suffix(name: &str) -> &str {
    name.strip_suffix(".git").unwrap_or(name)
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

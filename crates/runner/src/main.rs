//! Standalone poll-based CI runner binary.
//!
//! This is an OPTIONAL, ADDITIONAL execution path alongside the main
//! `server` binary's in-process `actions::Executor::run_job` calls (see the
//! `// DUAL-PATH:` comments in `crates/server/src/main.rs` and
//! `crates/graphql-api/src/mutation.rs`). Running this binary is not
//! required for CI jobs to execute -- the server still runs every job
//! in-process regardless. This binary polls the server's `/runner/claim`
//! HTTP endpoint for queued `runner_jobs` rows, executes them locally via
//! the same `actions::Executor`/`actions::Workflow` logic the server uses,
//! and reports completion back via `/runner/jobs/:id/complete`.
//!
//! Config (env vars):
//! - `GENOME_SERVER_URL` -- base URL of the main server, e.g. `http://localhost:8000`.
//! - `GENOME_RUNNER_TOKEN` -- bearer PAT used to authenticate against the server's
//!   `/runner/*` routes (validated the same way as any other bearer/PAT request).
//! - `GENOME_RUNNER_ARTIFACTS_DIR` -- local directory for artifact tarballs
//!   collected by `actions::Executor` (defaults to `./runner-artifacts`).
//! - `DOCKER_SOCKET_PATH` -- optional. If unset, connects to Docker via
//!   platform defaults (`DOCKER_HOST` env var, else the usual unix
//!   socket/named pipe), same as before. If set, connects to that exact
//!   socket path instead -- e.g. a rootless Podman socket -- so this
//!   process doesn't need root-equivalent access to the host's main Docker
//!   daemon. See the "Container isolation" section of the docs.
//! - `GENOME_RUNNER_ID` -- optional free-form identifier for this runner
//!   process, reported back on `dev_workspace_action` `create` jobs so
//!   `dev_workspaces.runner_id` records which runner is hosting a given
//!   workspace. Defaults to a freshly generated UUID if unset (so it's
//!   stable for this process's lifetime, but changes across restarts).
//!
//! This binary also claims and executes `dev_workspace_action` jobs (see
//! `dev_workspace_poll`), letting a dev workspace be hosted on this
//! runner's own Docker daemon instead of `server`'s. Only create/delete/exec
//! are wired up -- live port-proxying to a runner-hosted workspace isn't
//! supported yet (no reverse tunnel between runner and server exists), so
//! `server` rejects `start`/`stop`/proxy requests for one with a clear error
//! instead of silently trying the wrong Docker daemon.

use std::collections::HashMap;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

mod dev_workspace_poll;

#[derive(Debug, Deserialize)]
struct ClaimedJob {
    id: Uuid,
    kind: String,
    payload: String,
}

/// Mirrors the subset of a queued CI job's payload the runner needs:
/// the parsed job definition plus the repo archive (base64) to unpack into
/// the container's `/workspace`.
#[derive(Debug, Serialize, Deserialize)]
struct CiJobPayload {
    run_id: Uuid,
    job: actions::Job,
    #[serde(default)]
    repo_archive_b64: String,
    #[serde(default)]
    env_extra: HashMap<String, String>,
    #[serde(default)]
    secrets: HashMap<String, String>,
}

#[derive(Debug, Serialize)]
struct CompleteRequest {
    status: String,
    logs: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<String>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let server_url = std::env::var("GENOME_SERVER_URL")
        .map_err(|_| anyhow::anyhow!("GENOME_SERVER_URL environment variable must be set"))?;
    let runner_token = std::env::var("GENOME_RUNNER_TOKEN")
        .map_err(|_| anyhow::anyhow!("GENOME_RUNNER_TOKEN environment variable must be set"))?;
    let artifacts_dir = std::env::var("GENOME_RUNNER_ARTIFACTS_DIR")
        .unwrap_or_else(|_| "./runner-artifacts".to_string());

    let docker_socket_path = std::env::var("DOCKER_SOCKET_PATH").ok();

    let (executor, workspace_manager) = match &docker_socket_path {
        Some(socket) => {
            tracing::info!("connecting to Docker socket {socket} (from DOCKER_SOCKET_PATH)");
            (
                actions::Executor::new_with_socket(std::path::PathBuf::from(&artifacts_dir), socket)?,
                dev_env::WorkspaceManager::connect_socket(socket)?,
            )
        }
        None => (
            actions::Executor::new(std::path::PathBuf::from(&artifacts_dir))?,
            dev_env::WorkspaceManager::connect_local()?,
        ),
    };
    let runner_id = std::env::var("GENOME_RUNNER_ID").unwrap_or_else(|_| Uuid::new_v4().to_string());
    let http = reqwest::Client::new();

    tracing::info!("runner started (id={runner_id}), polling {server_url} every 5s");

    let mut interval = tokio::time::interval(Duration::from_secs(5));
    loop {
        interval.tick().await;

        match claim_job(&http, &server_url, &runner_token).await {
            Ok(Some(job)) => {
                if let Err(e) = handle_job(
                    &http,
                    &server_url,
                    &runner_token,
                    &executor,
                    &workspace_manager,
                    &runner_id,
                    job,
                )
                .await
                {
                    tracing::warn!("failed to handle claimed job: {e}");
                }
            }
            Ok(None) => {
                // No queued job available; nothing to do this tick.
            }
            Err(e) => {
                tracing::warn!("failed to poll /runner/claim: {e}");
            }
        }
    }
}

async fn claim_job(
    http: &reqwest::Client,
    server_url: &str,
    token: &str,
) -> anyhow::Result<Option<ClaimedJob>> {
    let resp = http
        .post(format!("{server_url}/runner/claim"))
        .bearer_auth(token)
        .send()
        .await?;

    if resp.status() == reqwest::StatusCode::NO_CONTENT {
        return Ok(None);
    }
    if !resp.status().is_success() {
        anyhow::bail!("claim request failed: {}", resp.status());
    }

    let job: ClaimedJob = resp.json().await?;
    Ok(Some(job))
}

#[allow(clippy::too_many_arguments)]
async fn handle_job(
    http: &reqwest::Client,
    server_url: &str,
    token: &str,
    executor: &actions::Executor,
    workspace_manager: &dev_env::WorkspaceManager,
    runner_id: &str,
    job: ClaimedJob,
) -> anyhow::Result<()> {
    if job.kind == entity_kind_dev_workspace() {
        let (status, logs, result) =
            dev_workspace_poll::handle_dev_workspace_job(workspace_manager, runner_id, &job.payload).await;
        return report_complete(http, server_url, token, job.id, status, logs, result).await;
    }

    let payload: CiJobPayload = serde_json::from_str(&job.payload)?;
    let archive = base64_decode(&payload.repo_archive_b64).unwrap_or_default();

    let logs = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
    let logs_clone = logs.clone();

    let result = executor
        .run_job(
            payload.run_id,
            &payload.job,
            &archive,
            payload.env_extra,
            &payload.secrets,
            move |line| {
                if let Ok(mut buf) = logs_clone.lock() {
                    buf.push_str(&line);
                    buf.push('\n');
                }
            },
        )
        .await;

    let collected_logs = logs.lock().map(|g| g.clone()).unwrap_or_default();

    let status = match result {
        Ok(job_result) => match job_result.status {
            actions::JobStatus::Success => "success",
            actions::JobStatus::Failure => "failure",
        },
        Err(e) => {
            tracing::warn!("job {} execution failed: {e}", job.id);
            "failure"
        }
    };

    report_complete(http, server_url, token, job.id, status, collected_logs, None).await
}

async fn report_complete(
    http: &reqwest::Client,
    server_url: &str,
    token: &str,
    job_id: Uuid,
    status: &str,
    logs: String,
    result: Option<String>,
) -> anyhow::Result<()> {
    let body = CompleteRequest {
        status: status.to_string(),
        logs,
        result,
    };
    let resp = http
        .post(format!("{server_url}/runner/jobs/{job_id}/complete"))
        .bearer_auth(token)
        .json(&body)
        .send()
        .await?;

    if !resp.status().is_success() {
        anyhow::bail!("complete request failed: {}", resp.status());
    }
    Ok(())
}

fn entity_kind_dev_workspace() -> &'static str {
    "dev_workspace_action"
}

/// Minimal base64 decode without pulling in a new dependency, matching the
/// small hand-rolled alphabet used elsewhere. Payloads are produced by the
/// server side, which is expected to use the standard `base64` crate; this
/// decodes standard (non-URL-safe) base64 with padding.
fn base64_decode(input: &str) -> Option<Vec<u8>> {
    if input.is_empty() {
        return Some(Vec::new());
    }
    use base64::Engine;
    base64::engine::general_purpose::STANDARD
        .decode(input)
        .ok()
}

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
//! - `DOCKER_SOCKET_PATH` -- accepted for forward-compatibility. `bollard`'s
//!   `Docker::connect_with_local_defaults()` (used by both `actions::Executor`
//!   and `dev_env::WorkspaceManager`) already honors Docker's own standard
//!   `DOCKER_HOST` env var / platform-default socket resolution; there is no
//!   separate bollard knob for an arbitrary "socket path" env var today, so
//!   this variable is currently read but only logged, not wired further.

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

    if let Ok(socket) = std::env::var("DOCKER_SOCKET_PATH") {
        tracing::info!(
            "DOCKER_SOCKET_PATH={socket} accepted for forward-compat; bollard resolves the \
             Docker socket via its own DOCKER_HOST convention / platform defaults, not this var"
        );
    }

    let executor = actions::Executor::new(std::path::PathBuf::from(&artifacts_dir))?;
    let http = reqwest::Client::new();

    tracing::info!("runner started, polling {server_url} every 5s");

    let mut interval = tokio::time::interval(Duration::from_secs(5));
    loop {
        interval.tick().await;

        match claim_job(&http, &server_url, &runner_token).await {
            Ok(Some(job)) => {
                if let Err(e) = handle_job(&http, &server_url, &runner_token, &executor, job).await
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

async fn handle_job(
    http: &reqwest::Client,
    server_url: &str,
    token: &str,
    executor: &actions::Executor,
    job: ClaimedJob,
) -> anyhow::Result<()> {
    if job.kind == entity_kind_dev_workspace() {
        // TODO(future work): full dev-workspace-hosting polling is not
        // implemented yet. `dev_workspace_poll::poll_dev_workspace_jobs`
        // exists as a stub proving `dev_env::WorkspaceManager` wiring
        // compiles; wire it up here once that feature is built out.
        tracing::warn!("received dev_workspace_action job {}; not yet supported by runner, skipping", job.id);
        report_complete(http, server_url, token, job.id, "failure", "dev_workspace_action jobs are not yet supported by the standalone runner".to_string()).await?;
        return Ok(());
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

    report_complete(http, server_url, token, job.id, status, collected_logs).await
}

async fn report_complete(
    http: &reqwest::Client,
    server_url: &str,
    token: &str,
    job_id: Uuid,
    status: &str,
    logs: String,
) -> anyhow::Result<()> {
    let body = CompleteRequest {
        status: status.to_string(),
        logs,
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

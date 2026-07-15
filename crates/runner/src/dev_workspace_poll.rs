//! Executes `dev_workspace_action` jobs claimed via `/runner/claim` against
//! this runner's own local Docker daemon (`dev_env::WorkspaceManager`).
//!
//! Known gap: the resulting container lives on *this* runner's Docker
//! daemon, not `server`'s. `server` has no network path back to it, so
//! live port-proxying (`GET /workspaces/:id/proxy/*path`) and
//! start/stop for runner-hosted workspaces are not supported yet (see the
//! matching restrictions in `graphql-api::mutation::create_dev_workspace`
//! and `crates/server/src/main.rs::proxy_workspace_request`) -- only
//! create/delete/exec are wired up here.

use serde::Deserialize;
use uuid::Uuid;

#[derive(Debug, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
enum DevWorkspaceJobPayload {
    Create {
        #[allow(dead_code)]
        workspace_id: Uuid,
        name: String,
        image: String,
        #[serde(default)]
        ports: Vec<(String, u16)>,
        #[serde(default)]
        repo_clone_url: Option<String>,
        owner: String,
    },
    Delete {
        #[allow(dead_code)]
        workspace_id: Uuid,
        container_id: String,
    },
    Exec {
        #[allow(dead_code)]
        workspace_id: Uuid,
        container_id: String,
        cmd: Vec<String>,
    },
}

/// Result of handling one dev-workspace job: `(status, logs, result_json)`,
/// matching the shape `/runner/jobs/:id/complete` expects (`status`/`logs`
/// from the existing CI-job path, plus the new optional `result`).
pub async fn handle_dev_workspace_job(
    workspace_manager: &dev_env::WorkspaceManager,
    runner_id: &str,
    payload_json: &str,
) -> (&'static str, String, Option<String>) {
    let payload = match serde_json::from_str::<DevWorkspaceJobPayload>(payload_json) {
        Ok(p) => p,
        Err(e) => {
            return ("failure", format!("failed to parse dev workspace job payload: {e}"), None);
        }
    };

    match payload {
        DevWorkspaceJobPayload::Create { name, image, ports, repo_clone_url, owner, .. } => {
            let port_refs: Vec<(&str, u16)> = ports.iter().map(|(n, p)| (n.as_str(), *p)).collect();
            match workspace_manager
                .create_workspace(&name, &image, &port_refs, repo_clone_url.as_deref(), None, None, &owner)
                .await
            {
                Ok(handle) => {
                    let result = serde_json::json!({
                        "container_id": handle.container_id,
                        "runner_id": runner_id,
                    });
                    (
                        "success",
                        format!("created workspace container {}", handle.container_id),
                        Some(result.to_string()),
                    )
                }
                Err(e) => ("failure", format!("failed to create workspace: {e}"), None),
            }
        }
        DevWorkspaceJobPayload::Delete { container_id, .. } => {
            match workspace_manager.delete_workspace(&container_id).await {
                Ok(()) => ("success", "workspace deleted".to_string(), None),
                Err(e) => ("failure", format!("failed to delete workspace: {e}"), None),
            }
        }
        DevWorkspaceJobPayload::Exec { container_id, cmd, .. } => {
            match workspace_manager.exec_command(&container_id, cmd).await {
                Ok(output) => {
                    let result = serde_json::json!({ "output": output });
                    ("success", "exec completed".to_string(), Some(result.to_string()))
                }
                Err(e) => ("failure", format!("exec failed: {e}"), None),
            }
        }
    }
}

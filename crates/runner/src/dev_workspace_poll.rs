//! Stub for future dev-workspace-hosting polling by the standalone runner.
//!
//! TODO(future work): the `runner_jobs.kind = 'dev_workspace_action'` column
//! value is already reserved (see `entity::runner_job::kind::DEV_WORKSPACE_ACTION`)
//! but no polling loop against `dev_env::WorkspaceManager` exists yet. This
//! function exists only to prove the dependency wiring compiles end-to-end;
//! it is not called from a real polling loop.
#[allow(dead_code)]
pub async fn poll_dev_workspace_jobs(workspace_manager: &dev_env::WorkspaceManager) {
    match workspace_manager.list_workspaces("").await {
        Ok(handles) => {
            tracing::debug!(
                "dev-workspace polling stub: {} existing workspace(s) known to Docker (no-op)",
                handles.len()
            );
        }
        Err(e) => {
            tracing::debug!("dev-workspace polling stub: failed to list workspaces: {e}");
        }
    }
}

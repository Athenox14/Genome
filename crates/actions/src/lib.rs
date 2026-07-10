//! # actions
//!
//! CI/CD execution engine for Genome, compatible with a practical subset of
//! GitHub Actions workflow YAML syntax so users can reuse their existing
//! `.github/workflows/*.yml` files.
//!
//! ## Compatibility scope
//!
//! Supported:
//! - `on:` triggers for `push`, `pull_request`, `workflow_dispatch`
//!   (both list-shorthand and map form with `branches`/`tags` filters).
//! - `jobs.<id>.runs-on`, `needs`, `env`, and `steps`.
//! - `steps[].run` — executed as a shell command inside a Docker container.
//! - `steps[].uses: actions/checkout@*` — no-op, since the repository
//!   contents are already materialized into the container's workspace.
//!
//! ## Limitations
//!
//! - There is **no GitHub Actions marketplace support**. Any `uses:` step
//!   other than `actions/checkout` is logged and skipped rather than
//!   executed — the job continues but that step is a no-op. This means
//!   workflows relying on actions like `actions/setup-node` or third-party
//!   actions will not get their expected side effects (e.g. toolchain
//!   installation); users should replace them with equivalent `run:` steps
//!   or pre-baked container images (`runs-on: <image>`).
//! - `strategy.matrix`, `outputs`, `if:` conditionals, reusable workflows,
//!   composite actions, and caching are not implemented.
//! - Windows/macOS runners are executed on a Linux Docker image fallback.

use std::collections::HashMap;

pub mod executor;
pub mod trigger;
pub mod workflow;

pub use executor::{Executor, JobResult, JobStatus};
pub use trigger::matches_event;
pub use workflow::{Job, Step, TriggerConfig, Workflow};

/// Errors that can occur while parsing or executing workflows.
#[derive(Debug, thiserror::Error)]
pub enum ActionsError {
    #[error("failed to parse workflow YAML: {0}")]
    Parse(#[source] serde_yaml::Error),

    #[error("docker error: {0}")]
    Docker(#[source] bollard::errors::Error),
}

/// Scan a map of repository file paths -> file contents for
/// `.github/workflows/*.yml` / `*.yaml` files and parse each one.
///
/// Files that fail to parse are skipped (with a `tracing::warn!` log)
/// rather than causing the whole batch to fail, so a single malformed
/// workflow does not block CI for the rest of the repository.
pub fn discover_workflows(repo_files: &HashMap<String, Vec<u8>>) -> Vec<(String, Workflow)> {
    let mut found = Vec::new();

    for (path, contents) in repo_files {
        let normalized = path.replace('\\', "/");
        if !normalized.starts_with(".github/workflows/") {
            continue;
        }
        let is_yaml = normalized.ends_with(".yml") || normalized.ends_with(".yaml");
        if !is_yaml {
            continue;
        }

        let text = match std::str::from_utf8(contents) {
            Ok(t) => t,
            Err(e) => {
                tracing::warn!("workflow {path} is not valid UTF-8: {e}");
                continue;
            }
        };

        match Workflow::parse(text) {
            Ok(workflow) => found.push((path.clone(), workflow)),
            Err(e) => {
                tracing::warn!("failed to parse workflow {path}: {e}");
            }
        }
    }

    found
}

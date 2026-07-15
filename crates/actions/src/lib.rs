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
//! - `steps[].uses: docker://image` and `owner/repo[/path]@ref` marketplace
//!   actions whose `action.yml` declares `runs.using` as `docker`,
//!   `composite`, or a Node runtime (`node12`/`node16`/`node18`/`node20`)
//!   — see `marketplace` and `Executor::execute_uses_step`. Composite
//!   actions may not nest another composite action (one level deep only).
//!
//! ## Limitations
//!
//! - Any `uses:` value that isn't `actions/checkout`, a `docker://` image,
//!   or a resolvable `owner/repo[/path]@ref` (or one whose `action.yml`
//!   declares an unsupported `runs.using`, or a nested composite action) is
//!   logged and skipped rather than executed — the job continues but that
//!   step is a no-op.
//! - `strategy.matrix`, `outputs`, `if:` conditionals, reusable workflows,
//!   and caching are not implemented.
//! - Windows/macOS runners are executed on a Linux Docker image fallback.

use std::collections::HashMap;

pub mod executor;
pub mod marketplace;
pub mod secrets;
pub mod trigger;
pub mod workflow;

pub use executor::{ArtifactMeta, Executor, JobResult, JobStatus};
pub use secrets::{decrypt_secret, encrypt_secret, key_from_base64, substitute_secrets};
pub use trigger::matches_event;
pub use workflow::{Job, Step, TriggerConfig, Workflow};

/// Errors that can occur while parsing or executing workflows.
#[derive(Debug, thiserror::Error)]
pub enum ActionsError {
    #[error("failed to parse workflow YAML: {0}")]
    Parse(#[source] serde_yaml::Error),

    #[error("docker error: {0}")]
    Docker(#[source] bollard::errors::Error),

    #[error("artifact error: {0}")]
    Artifact(String),

    #[error("marketplace action error: {0}")]
    Marketplace(String),
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

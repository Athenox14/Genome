use std::collections::HashMap;
use std::path::Path;

use async_trait::async_trait;
use git2::Repository;

use crate::error::Result;

/// Implemented by server-layer components that want to react to pushes
/// (e.g. to trigger CI/CD runs). `git-core` itself never invokes hooks
/// automatically; the caller is expected to snapshot refs before and after
/// running the receive-pack subprocess (via [`diff_refs`]) and invoke the
/// sink for each ref that changed.
#[async_trait]
pub trait CommitHookSink: Send + Sync {
    async fn on_push(&self, owner: &str, repo: &str, ref_name: &str, old_sha: &str, new_sha: &str);
}

/// Snapshot every reference in a repository as a map of fully-qualified ref
/// name (e.g. `refs/heads/main`) to the SHA it currently points at. Useful
/// for taking a "before" and "after" snapshot around a receive-pack call so
/// the caller can compute which refs changed and invoke a [`CommitHookSink`].
pub fn diff_refs(repo_path: &Path) -> Result<HashMap<String, String>> {
    let repo = Repository::open_bare(repo_path)?;
    let mut out = HashMap::new();
    for reference in repo.references()? {
        let reference = reference?;
        if let (Some(name), Some(oid)) = (reference.name(), reference.target()) {
            out.insert(name.to_string(), oid.to_string());
        }
    }
    Ok(out)
}

/// Compute which refs changed (added, updated, or deleted) between a
/// "before" and "after" snapshot produced by [`diff_refs`]. Returns tuples of
/// `(ref_name, old_sha, new_sha)`, using the all-zero SHA to represent a ref
/// that did not exist before/after.
pub fn compute_ref_changes(
    before: &HashMap<String, String>,
    after: &HashMap<String, String>,
) -> Vec<(String, String, String)> {
    const ZERO_SHA: &str = "0000000000000000000000000000000000000000";
    let mut changes = Vec::new();

    for (name, new_sha) in after {
        let old_sha = before.get(name).cloned().unwrap_or_else(|| ZERO_SHA.to_string());
        if &old_sha != new_sha {
            changes.push((name.clone(), old_sha, new_sha.clone()));
        }
    }
    for (name, old_sha) in before {
        if !after.contains_key(name) {
            changes.push((name.clone(), old_sha.clone(), ZERO_SHA.to_string()));
        }
    }
    changes
}

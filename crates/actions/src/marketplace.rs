//! Support for a practical subset of the GitHub Actions marketplace:
//! parsing a `uses:` reference, fetching the referenced action's source
//! from GitHub, and reading its `action.yml`/`action.yaml`.
//!
//! Deliberately out of scope (see `ActionRef::parse`/`Executor::execute_uses_step`):
//! anything that doesn't parse as `owner/repo[/path]@ref` or `docker://image`,
//! and (to avoid unbounded recursion) composite actions nested inside
//! another composite action.

use std::collections::HashMap;
use std::path::PathBuf;

use serde::Deserialize;

use crate::workflow::Step;
use crate::ActionsError;

/// A parsed `uses:` reference, minus the two special-cased values handled
/// directly by the executor (`actions/checkout@*`, which is a no-op, and
/// bare `genome/upload-artifact`, which has no `@ref` and so never parses
/// here at all -- it falls through to `ActionRef::parse` returning `None`,
/// same as any other unversioned/malformed `uses:` value).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActionRef {
    /// `docker://image[:tag]` -- run that image directly, no action.yml.
    DockerImage(String),
    /// `owner/repo[/path]@ref` -- a marketplace action to fetch from GitHub.
    Marketplace {
        owner: String,
        repo: String,
        path: Option<String>,
        git_ref: String,
    },
}

impl ActionRef {
    pub fn parse(uses: &str) -> Option<ActionRef> {
        if let Some(image) = uses.strip_prefix("docker://") {
            return Some(ActionRef::DockerImage(image.to_string()));
        }
        let (path_part, git_ref) = uses.split_once('@')?;
        if path_part.is_empty() || git_ref.is_empty() {
            return None;
        }
        let mut segments = path_part.splitn(3, '/');
        let owner = segments.next()?.to_string();
        let repo = segments.next()?.to_string();
        if owner.is_empty() || repo.is_empty() {
            return None;
        }
        let path = segments.next().filter(|s| !s.is_empty()).map(|s| s.to_string());
        Some(ActionRef::Marketplace {
            owner,
            repo,
            path,
            git_ref: git_ref.to_string(),
        })
    }
}

/// Mirrors the subset of `action.yml`/`action.yaml` we understand.
#[derive(Debug, Clone, Deserialize)]
pub struct ActionMetadata {
    #[serde(default)]
    pub inputs: HashMap<String, ActionInput>,
    pub runs: ActionRuns,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ActionInput {
    #[serde(default)]
    pub default: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ActionRuns {
    /// `"docker"`, `"composite"`, or a JS runtime like `"node20"`.
    pub using: String,
    /// Docker actions only: `"docker://image:tag"` or a path to a
    /// `Dockerfile` (relative to the action's directory) to build.
    #[serde(default)]
    pub image: Option<String>,
    #[serde(default)]
    pub entrypoint: Option<String>,
    #[serde(default)]
    pub args: Option<Vec<String>>,
    /// JS actions only: entry script, relative to the action's directory.
    #[serde(default)]
    pub main: Option<String>,
    /// Composite actions only. Reuses the workflow `Step` shape since the
    /// fields composite steps support (`run`/`uses`/`with`/`env`/`shell`)
    /// are the same ones a job step supports.
    #[serde(default)]
    pub steps: Option<Vec<Step>>,
}

/// A fetched action: its checked-out source directory and parsed metadata.
/// Holds on to the `TempDir` so it isn't deleted while `dir` is still in use.
pub struct FetchedAction {
    pub dir: PathBuf,
    pub metadata: ActionMetadata,
    _tempdir: tempfile::TempDir,
}

/// Shallow-clone-equivalent fetch of a marketplace action: clones the
/// referenced GitHub repo (git2 doesn't make a true `--depth 1` clone
/// straightforward when the target ref is a tag/branch other than the
/// remote's HEAD, so this is a full clone -- action repos are small, so
/// the cost is acceptable), resolves `git_ref` against tags/branches/raw
/// SHAs, checks out that commit, and parses `action.yml`/`action.yaml`
/// from the (optional) `path` subdirectory.
pub fn fetch_action(
    owner: &str,
    repo: &str,
    path: Option<&str>,
    git_ref: &str,
) -> Result<FetchedAction, ActionsError> {
    let tempdir = tempfile::tempdir().map_err(|e| ActionsError::Marketplace(e.to_string()))?;
    let url = format!("https://github.com/{owner}/{repo}.git");

    let git_repo = git2::Repository::clone(&url, tempdir.path())
        .map_err(|e| ActionsError::Marketplace(format!("failed to clone {url}: {e}")))?;

    let oid = resolve_git_ref(&git_repo, git_ref)?;
    git_repo
        .set_head_detached(oid)
        .map_err(|e| ActionsError::Marketplace(format!("failed to check out {git_ref}: {e}")))?;
    let mut checkout = git2::build::CheckoutBuilder::new();
    checkout.force();
    git_repo
        .checkout_head(Some(&mut checkout))
        .map_err(|e| ActionsError::Marketplace(format!("failed to check out {git_ref}: {e}")))?;
    drop(git_repo);

    let action_dir = match path {
        Some(p) => tempdir.path().join(p),
        None => tempdir.path().to_path_buf(),
    };

    let yml_path = ["action.yml", "action.yaml"]
        .iter()
        .map(|f| action_dir.join(f))
        .find(|p| p.exists())
        .ok_or_else(|| {
            ActionsError::Marketplace(format!("no action.yml/action.yaml found in {owner}/{repo}{}",
                path.map(|p| format!("/{p}")).unwrap_or_default()))
        })?;
    let text = std::fs::read_to_string(&yml_path).map_err(|e| ActionsError::Marketplace(e.to_string()))?;
    let metadata: ActionMetadata = serde_yaml::from_str(&text)
        .map_err(|e| ActionsError::Marketplace(format!("failed to parse {}: {e}", yml_path.display())))?;

    Ok(FetchedAction {
        dir: action_dir,
        metadata,
        _tempdir: tempdir,
    })
}

fn resolve_git_ref(repo: &git2::Repository, git_ref: &str) -> Result<git2::Oid, ActionsError> {
    for candidate in [format!("refs/tags/{git_ref}"), format!("refs/remotes/origin/{git_ref}")] {
        if let Ok(reference) = repo.find_reference(&candidate) {
            if let Some(oid) = reference.target() {
                return Ok(oid);
            }
        }
    }
    repo.revparse_single(git_ref)
        .and_then(|obj| obj.peel_to_commit())
        .map(|c| c.id())
        .map_err(|e| ActionsError::Marketplace(format!("could not resolve ref '{git_ref}': {e}")))
}

/// Resolve final input values: `with:` overrides `action.yml`'s declared
/// defaults. `metadata` is `None` for a direct `docker://image` step (no
/// action.yml at all), in which case only `with:` values are used.
pub fn resolve_inputs(
    metadata: Option<&ActionMetadata>,
    with: Option<&HashMap<String, serde_yaml::Value>>,
) -> HashMap<String, String> {
    let mut resolved = HashMap::new();
    if let Some(with) = with {
        for (k, v) in with {
            let value = match v {
                serde_yaml::Value::String(s) => s.clone(),
                serde_yaml::Value::Bool(b) => b.to_string(),
                serde_yaml::Value::Number(n) => n.to_string(),
                other => serde_yaml::to_string(other).unwrap_or_default().trim().to_string(),
            };
            resolved.insert(k.clone(), value);
        }
    }
    if let Some(metadata) = metadata {
        for (name, input) in &metadata.inputs {
            if !resolved.contains_key(name) {
                if let Some(default) = &input.default {
                    resolved.insert(name.clone(), default.clone());
                }
            }
        }
    }
    resolved
}

/// GitHub Actions exposes each input `foo-bar` to the action's process as
/// `INPUT_FOO-BAR` (uppercased, spaces -> underscores; hyphens are left
/// as-is). This is a close approximation of that convention.
fn input_env_name(name: &str) -> String {
    format!("INPUT_{}", name.to_uppercase().replace(' ', "_"))
}

pub fn input_env_vars(resolved: &HashMap<String, String>) -> Vec<String> {
    resolved
        .iter()
        .map(|(k, v)| format!("{}={}", input_env_name(k), v))
        .collect()
}

/// Substitutes `${{ inputs.NAME }}` (with or without inner spaces) in
/// `runs.args`/`runs.entrypoint` templating for Docker actions.
pub fn substitute_inputs(text: &str, resolved: &HashMap<String, String>) -> String {
    let mut out = text.to_string();
    for (k, v) in resolved {
        out = out.replace(&format!("${{{{ inputs.{k} }}}}"), v);
        out = out.replace(&format!("${{{{inputs.{k}}}}}"), v);
    }
    out
}

/// Maps a JS action's declared `runs.using` runtime to a Docker image that
/// actually has that Node version, since (unlike a real GitHub-hosted
/// runner) the job's own `runs-on` image has no reason to include Node.
pub fn node_image_for_using(using: &str) -> &'static str {
    match using {
        "node12" => "node:12-slim",
        "node16" => "node:16-slim",
        "node18" => "node:18-slim",
        _ => "node:20-slim",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_docker_image_ref() {
        assert_eq!(
            ActionRef::parse("docker://alpine:3.19"),
            Some(ActionRef::DockerImage("alpine:3.19".to_string()))
        );
    }

    #[test]
    fn parses_marketplace_ref_without_path() {
        assert_eq!(
            ActionRef::parse("actions/setup-node@v4"),
            Some(ActionRef::Marketplace {
                owner: "actions".to_string(),
                repo: "setup-node".to_string(),
                path: None,
                git_ref: "v4".to_string(),
            })
        );
    }

    #[test]
    fn parses_marketplace_ref_with_path() {
        assert_eq!(
            ActionRef::parse("actions/aws/ec2@main"),
            Some(ActionRef::Marketplace {
                owner: "actions".to_string(),
                repo: "aws".to_string(),
                path: Some("ec2".to_string()),
                git_ref: "main".to_string(),
            })
        );
    }

    #[test]
    fn rejects_unversioned_ref() {
        assert_eq!(ActionRef::parse("genome/upload-artifact"), None);
    }

    #[test]
    fn resolves_with_overriding_default() {
        let mut inputs = HashMap::new();
        inputs.insert(
            "foo".to_string(),
            ActionInput {
                default: Some("default-value".to_string()),
            },
        );
        let metadata = ActionMetadata {
            inputs,
            runs: ActionRuns {
                using: "docker".to_string(),
                image: None,
                entrypoint: None,
                args: None,
                main: None,
                steps: None,
            },
        };
        let mut with = HashMap::new();
        with.insert("foo".to_string(), serde_yaml::Value::String("override".to_string()));

        let resolved = resolve_inputs(Some(&metadata), Some(&with));
        assert_eq!(resolved.get("foo"), Some(&"override".to_string()));
    }

    #[test]
    fn substitutes_input_placeholders() {
        let mut resolved = HashMap::new();
        resolved.insert("name".to_string(), "world".to_string());
        assert_eq!(
            substitute_inputs("hello ${{ inputs.name }}", &resolved),
            "hello world"
        );
    }
}

use std::collections::HashMap;

use bollard::container::{
    Config, CreateContainerOptions, ListContainersOptions, RemoveContainerOptions,
    StartContainerOptions, StopContainerOptions,
};
use bollard::exec::{CreateExecOptions, StartExecResults};
use bollard::image::CreateImageOptions;
use bollard::models::{HostConfig, PortBinding};
use bollard::Docker;
use futures_util::StreamExt;
use rand::Rng;
use serde::{Deserialize, Serialize};

use crate::{DevEnvError, Result};

/// Default image providing a browser-based VS Code (code-server) UI.
pub const IMAGE_CODE_SERVER: &str = "codercom/code-server:latest";
/// Plain Ubuntu image, useful for SSH-only workspaces.
pub const IMAGE_UBUNTU: &str = "ubuntu:22.04";

const LABEL_WORKSPACE: &str = "genome.workspace";
const LABEL_OWNER: &str = "genome.owner";
/// Prefix used for per-container-port labels, e.g. `genome.port.http=8080`,
/// so that named port -> container port mappings survive a daemon restart
/// and can be reconstructed purely from `docker inspect`/`list_containers`.
const LABEL_PORT_PREFIX: &str = "genome.port.";
/// Fallback port binding used when a caller supplies a raw `image` with no
/// explicit port list, preserving the pre-template single-port behavior.
const DEFAULT_PORT_NAME: &str = "http";
const DEFAULT_CONTAINER_PORT: u16 = 8080;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceHandle {
    pub container_id: String,
    /// Named container port -> host port, e.g. `{"http": 41231}`.
    pub ports: HashMap<String, u16>,
    pub name: String,
}

impl WorkspaceHandle {
    /// Returns the host port for the given named port, if bound.
    pub fn port(&self, name: &str) -> Option<u16> {
        self.ports.get(name).copied()
    }

    /// Returns the "http" port if present, else the first bound port
    /// (arbitrary but stable given a `HashMap` iteration for a single-entry
    /// map, which is the common case).
    pub fn default_port(&self) -> Option<u16> {
        self.ports
            .get(DEFAULT_PORT_NAME)
            .copied()
            .or_else(|| self.ports.values().next().copied())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WorkspaceStatus {
    Running,
    Stopped,
    NotFound,
}

pub struct WorkspaceManager {
    docker: Docker,
}

impl WorkspaceManager {
    pub fn new(docker: Docker) -> Self {
        Self { docker }
    }

    /// Connects to the local Docker daemon using the platform default socket / pipe.
    pub fn connect_local() -> Result<Self> {
        let docker = Docker::connect_with_local_defaults()?;
        Ok(Self { docker })
    }

    /// Connects to a specific Docker (or Docker-API-compatible, e.g.
    /// rootless Podman) socket path, honoring a configured
    /// `DOCKER_SOCKET_PATH` instead of always resolving the platform
    /// default. Pointing this at a rootless engine's socket instead of the
    /// default `/var/run/docker.sock` is the supported way to run without
    /// giving this process root-equivalent access to the host's main
    /// Docker daemon -- see the "Container isolation" section of the docs.
    pub fn connect_socket(path: &str) -> Result<Self> {
        let docker = Docker::connect_with_socket(path, 120, bollard::API_DEFAULT_VERSION)?;
        Ok(Self { docker })
    }

    /// Validates a workspace name: lowercase alphanumeric and hyphens only, must start
    /// with a letter, 1-63 chars. This becomes part of the container name so it must be
    /// safe to pass to the Docker API.
    fn validate_name(name: &str) -> Result<()> {
        let valid = !name.is_empty()
            && name.len() <= 63
            && name
                .chars()
                .next()
                .map(|c| c.is_ascii_alphabetic())
                .unwrap_or(false)
            && name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');

        if valid {
            Ok(())
        } else {
            Err(DevEnvError::InvalidName(name.to_string()))
        }
    }

    fn random_host_port() -> u16 {
        rand::thread_rng().gen_range(20000..60000)
    }

    /// Creates and starts a new workspace container.
    ///
    /// * `image` - the Docker image to run (e.g. [`IMAGE_CODE_SERVER`] or [`IMAGE_UBUNTU`]).
    /// * `ports` - named container ports to expose (name, container port), e.g.
    ///   `[("http", 8080)]`. If empty, falls back to a single `("http", 8080)`
    ///   binding for backward compatibility with raw-image callers.
    /// * `repo_clone_url` - if provided, the repo is cloned into `/home/coder/project`
    ///   before the container's default process starts.
    /// * `cpu_limit` - fractional CPU limit (e.g. `2.0` for two cores).
    /// * `mem_limit_mb` - memory limit in megabytes.
    pub async fn create_workspace(
        &self,
        name: &str,
        image: &str,
        ports: &[(&str, u16)],
        repo_clone_url: Option<&str>,
        cpu_limit: Option<f64>,
        mem_limit_mb: Option<i64>,
        owner: &str,
    ) -> Result<WorkspaceHandle> {
        Self::validate_name(name)?;
        if image.trim().is_empty() {
            return Err(DevEnvError::InvalidRequest(
                "image must not be empty".to_string(),
            ));
        }

        let ports: Vec<(String, u16)> = if ports.is_empty() {
            vec![(DEFAULT_PORT_NAME.to_string(), DEFAULT_CONTAINER_PORT)]
        } else {
            ports.iter().map(|(n, p)| (n.to_string(), *p)).collect()
        };

        let mut port_bindings = HashMap::new();
        let mut exposed_ports = HashMap::new();
        let mut host_ports = HashMap::new();
        let mut labels = HashMap::new();
        labels.insert(LABEL_WORKSPACE.to_string(), "true".to_string());
        labels.insert(LABEL_OWNER.to_string(), owner.to_string());

        for (port_name, container_port) in &ports {
            let host_port = Self::random_host_port();
            let key = format!("{container_port}/tcp");
            port_bindings.insert(
                key.clone(),
                Some(vec![PortBinding {
                    host_ip: Some("0.0.0.0".to_string()),
                    host_port: Some(host_port.to_string()),
                }]),
            );
            exposed_ports.insert(key, HashMap::new());
            labels.insert(format!("{LABEL_PORT_PREFIX}{port_name}"), container_port.to_string());
            host_ports.insert(port_name.clone(), host_port);
        }

        let host_config = HostConfig {
            nano_cpus: cpu_limit.map(|c| (c * 1_000_000_000.0) as i64),
            memory: mem_limit_mb.map(|m| m * 1024 * 1024),
            port_bindings: Some(port_bindings),
            ..Default::default()
        };

        // If a repo URL is supplied, wrap the container's normal startup in a shell
        // command that first clones the repo, then hands off to whatever the base
        // image would normally run.
        let entrypoint = repo_clone_url.map(|url| {
            let clone_cmd = format!(
                "git clone {} /home/coder/project || true; exec \"$@\"",
                shell_escape(url)
            );
            vec![
                "/bin/sh".to_string(),
                "-c".to_string(),
                clone_cmd,
                "--".to_string(),
            ]
        });

        // The default command that runs code-server (or, for a plain ubuntu image,
        // just sleeps so the container stays up for SSH access).
        let cmd = if image == IMAGE_CODE_SERVER {
            Some(vec![
                "code-server".to_string(),
                "--bind-addr".to_string(),
                "0.0.0.0:8080".to_string(),
                "--auth".to_string(),
                "none".to_string(),
                "/home/coder/project".to_string(),
            ])
        } else {
            Some(vec!["sleep".to_string(), "infinity".to_string()])
        };

        let config = Config {
            image: Some(image.to_string()),
            entrypoint,
            cmd,
            exposed_ports: Some(exposed_ports),
            host_config: Some(host_config),
            labels: Some(labels),
            ..Default::default()
        };

        let options = CreateContainerOptions {
            name: name.to_string(),
            platform: None,
        };

        // Ensure the image is present locally before creating the container. Pulling
        // an already-present image is a no-op, so we always attempt it rather than
        // trying to detect whether it's needed first.
        {
            let mut stream = self.docker.create_image(
                Some(CreateImageOptions {
                    from_image: image.to_string(),
                    ..Default::default()
                }),
                None,
                None,
            );
            while let Some(item) = stream.next().await {
                item.map_err(DevEnvError::Docker)?;
            }
        }

        let created = self
            .docker
            .create_container(Some(options), config)
            .await?;

        self.docker
            .start_container(&created.id, None::<StartContainerOptions<String>>)
            .await?;

        Ok(WorkspaceHandle {
            container_id: created.id,
            ports: host_ports,
            name: name.to_string(),
        })
    }

    /// Stops (but does not remove) a workspace container so it can be resumed later.
    pub async fn stop_workspace(&self, container_id: &str) -> Result<()> {
        self.docker
            .stop_container(container_id, None::<StopContainerOptions>)
            .await?;
        Ok(())
    }

    /// Resumes a previously stopped workspace.
    pub async fn start_workspace(&self, container_id: &str) -> Result<()> {
        self.docker
            .start_container(container_id, None::<StartContainerOptions<String>>)
            .await?;
        Ok(())
    }

    /// Stops and permanently removes a workspace container.
    pub async fn delete_workspace(&self, container_id: &str) -> Result<()> {
        let _ = self
            .docker
            .stop_container(container_id, None::<StopContainerOptions>)
            .await;

        self.docker
            .remove_container(
                container_id,
                Some(RemoveContainerOptions {
                    force: true,
                    ..Default::default()
                }),
            )
            .await?;
        Ok(())
    }

    /// Returns the current status of a workspace container.
    pub async fn workspace_status(&self, container_id: &str) -> Result<WorkspaceStatus> {
        match self.docker.inspect_container(container_id, None).await {
            Ok(inspect) => {
                let running = inspect
                    .state
                    .and_then(|s| s.running)
                    .unwrap_or(false);
                Ok(if running {
                    WorkspaceStatus::Running
                } else {
                    WorkspaceStatus::Stopped
                })
            }
            Err(bollard::errors::Error::DockerResponseServerError {
                status_code: 404, ..
            }) => Ok(WorkspaceStatus::NotFound),
            Err(e) => Err(DevEnvError::Docker(e)),
        }
    }

    /// Lists all workspace containers created by Genome, optionally filtered by owner.
    pub async fn list_workspaces(&self, label_filter: &str) -> Result<Vec<WorkspaceHandle>> {
        let mut filters = HashMap::new();
        filters.insert("label".to_string(), vec![LABEL_WORKSPACE.to_string()]);
        if !label_filter.is_empty() {
            filters.insert(
                "label".to_string(),
                vec![
                    LABEL_WORKSPACE.to_string(),
                    format!("{}={}", LABEL_OWNER, label_filter),
                ],
            );
        }

        let options = ListContainersOptions {
            all: true,
            filters,
            ..Default::default()
        };

        let containers = self.docker.list_containers(Some(options)).await?;

        let mut handles = Vec::new();
        for c in containers {
            let container_id = c.id.unwrap_or_default();
            let name = c
                .names
                .as_ref()
                .and_then(|n| n.first())
                .map(|n| n.trim_start_matches('/').to_string())
                .unwrap_or_default();

            let container_ports = c.ports.unwrap_or_default();
            let find_host_port = |container_port: u16| {
                container_ports
                    .iter()
                    .find(|p| p.private_port == container_port)
                    .and_then(|p| p.public_port)
            };

            let mut ports = HashMap::new();
            let port_labels: Vec<(String, u16)> = c
                .labels
                .as_ref()
                .map(|labels| {
                    labels
                        .iter()
                        .filter_map(|(k, v)| {
                            k.strip_prefix(LABEL_PORT_PREFIX)
                                .and_then(|port_name| v.parse::<u16>().ok().map(|p| (port_name.to_string(), p)))
                        })
                        .collect()
                })
                .unwrap_or_default();

            if port_labels.is_empty() {
                // Container predates named-port labels (or has none): fall back to the
                // legacy single default-port lookup.
                if let Some(host_port) = find_host_port(DEFAULT_CONTAINER_PORT) {
                    ports.insert(DEFAULT_PORT_NAME.to_string(), host_port);
                }
            } else {
                for (port_name, container_port) in port_labels {
                    if let Some(host_port) = find_host_port(container_port) {
                        ports.insert(port_name, host_port);
                    }
                }
            }

            handles.push(WorkspaceHandle {
                container_id,
                ports,
                name,
            });
        }

        Ok(handles)
    }

    /// Runs an arbitrary command inside a workspace container and returns its combined
    /// stdout/stderr output. Useful for e.g. installing an SSH server post-creation.
    pub async fn exec_command(&self, container_id: &str, cmd: Vec<String>) -> Result<String> {
        if cmd.is_empty() {
            return Err(DevEnvError::InvalidRequest(
                "cmd must not be empty".to_string(),
            ));
        }

        let exec = self
            .docker
            .create_exec(
                container_id,
                CreateExecOptions {
                    attach_stdout: Some(true),
                    attach_stderr: Some(true),
                    cmd: Some(cmd),
                    ..Default::default()
                },
            )
            .await?;

        let start = self.docker.start_exec(&exec.id, None).await?;

        let mut output = String::new();
        if let StartExecResults::Attached { output: mut stream, .. } = start {
            while let Some(chunk) = stream.next().await {
                match chunk {
                    Ok(log) => {
                        let log: bollard::container::LogOutput = log;
                        output.push_str(&log.to_string());
                    }
                    Err(e) => return Err(DevEnvError::Docker(e)),
                }
            }
        }

        Ok(output)
    }
}

/// Very small shell-escaping helper for interpolating a URL into a `/bin/sh -c` string.
/// Wraps the value in single quotes and escapes any embedded single quotes.
fn shell_escape(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

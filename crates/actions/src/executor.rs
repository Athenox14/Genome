//! Docker-based execution of a single workflow job via `bollard`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use bollard::container::{
    Config, CreateContainerOptions, DownloadFromContainerOptions, LogsOptions,
    RemoveContainerOptions, UploadToContainerOptions, WaitContainerOptions,
};
use bollard::exec::{CreateExecOptions, StartExecResults};
use bollard::image::{BuildImageOptions, CreateImageOptions};
use bollard::Docker;
use futures_util::StreamExt;
use uuid::Uuid;

use crate::marketplace;
use crate::workflow::{Job, Step};
use crate::ActionsError;

/// Final status of a job run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobStatus {
    Success,
    Failure,
}

/// Metadata for a single artifact collected from a job's container after a
/// successful run, produced by a `uses: genome/upload-artifact` step.
#[derive(Debug, Clone)]
pub struct ArtifactMeta {
    pub name: String,
    pub file_path: String,
    pub size_bytes: i64,
}

/// Result of running a job to completion.
#[derive(Debug, Clone)]
pub struct JobResult {
    pub status: JobStatus,
    pub exit_code: i32,
    pub artifacts: Vec<ArtifactMeta>,
}

/// Executes workflow jobs inside disposable Docker containers.
pub struct Executor {
    docker: Docker,
    /// Base directory under which per-run artifact tarballs are written:
    /// `{artifacts_root}/{run_id}/{artifact_name}.tar`.
    artifacts_root: PathBuf,
}

impl Executor {
    /// Connect to the local Docker daemon using platform defaults
    /// (named pipe on Windows, unix socket elsewhere).
    pub fn new(artifacts_root: PathBuf) -> Result<Executor, ActionsError> {
        let docker =
            Docker::connect_with_local_defaults().map_err(ActionsError::Docker)?;
        Ok(Executor { docker, artifacts_root })
    }

    /// Connect to a specific Docker (or Docker-API-compatible, e.g.
    /// rootless Podman) socket path rather than always resolving the
    /// platform default -- see `dev_env::WorkspaceManager::connect_socket`
    /// for why this matters for running without a host-Docker-socket
    /// mount.
    pub fn new_with_socket(artifacts_root: PathBuf, docker_socket_path: &str) -> Result<Executor, ActionsError> {
        let docker = Docker::connect_with_socket(docker_socket_path, 120, bollard::API_DEFAULT_VERSION)
            .map_err(ActionsError::Docker)?;
        Ok(Executor { docker, artifacts_root })
    }

    /// Map a `runs-on:` value to a concrete Docker image tag.
    fn image_for_runs_on(runs_on: &str) -> String {
        match runs_on {
            "ubuntu-latest" | "ubuntu-22.04" => "ubuntu:22.04".to_string(),
            "ubuntu-20.04" => "ubuntu:20.04".to_string(),
            "node-latest" => "node:20".to_string(),
            "windows-latest" | "macos-latest" => {
                // Not natively supported by a Linux Docker host; fall back
                // to a generic runner image rather than failing outright.
                "ubuntu:22.04".to_string()
            }
            other if other.contains(':') || other.contains('/') => other.to_string(),
            _ => "ubuntu:22.04".to_string(),
        }
    }

    /// Run a single job to completion inside a fresh container.
    ///
    /// `workdir_repo_archive` must be a tar byte stream that will be
    /// uploaded (unpacked) into `/workspace` inside the container.
    /// `log_sink` receives each line of combined stdout/stderr output as it
    /// is produced.
    pub async fn run_job(
        &self,
        run_id: Uuid,
        job: &Job,
        workdir_repo_archive: &[u8],
        env_extra: HashMap<String, String>,
        secrets: &HashMap<String, String>,
        log_sink: impl Fn(String) + Send + Sync + 'static,
    ) -> Result<JobResult, ActionsError> {
        let image = Self::image_for_runs_on(&job.runs_on);

        // Best-effort image pull; ignore errors if the image already
        // exists locally (e.g. offline dev environments).
        {
            let mut stream = self.docker.create_image(
                Some(CreateImageOptions {
                    from_image: image.clone(),
                    ..Default::default()
                }),
                None,
                None,
            );
            while let Some(item) = stream.next().await {
                if let Err(e) = item {
                    tracing::warn!("image pull warning for {image}: {e}");
                    break;
                }
            }
        }

        let container_name = format!("genome-job-{}", uuid::Uuid::new_v4());
        let mut env_vars: Vec<String> = Vec::new();
        if let Some(job_env) = &job.env {
            for (k, v) in job_env {
                env_vars.push(format!("{k}={v}"));
            }
        }
        for (k, v) in &env_extra {
            env_vars.push(format!("{k}={v}"));
        }

        let config = Config {
            image: Some(image.clone()),
            cmd: Some(vec![
                "sh".to_string(),
                "-c".to_string(),
                "mkdir -p /workspace && tail -f /dev/null".to_string(),
            ]),
            env: Some(env_vars.clone()),
            working_dir: Some("/workspace".to_string()),
            tty: Some(true),
            ..Default::default()
        };

        self.docker
            .create_container(
                Some(CreateContainerOptions {
                    name: container_name.clone(),
                    platform: None,
                }),
                config,
            )
            .await
            .map_err(ActionsError::Docker)?;

        self.docker
            .start_container::<String>(&container_name, None)
            .await
            .map_err(ActionsError::Docker)?;

        let cleanup = |docker: Docker, name: String| async move {
            let _ = docker
                .remove_container(
                    &name,
                    Some(RemoveContainerOptions {
                        force: true,
                        ..Default::default()
                    }),
                )
                .await;
        };

        if let Err(e) = self
            .docker
            .upload_to_container(
                &container_name,
                Some(UploadToContainerOptions {
                    path: "/workspace",
                    no_overwrite_dir_non_dir: "false",
                }),
                workdir_repo_archive.to_vec().into(),
            )
            .await
        {
            cleanup(self.docker.clone(), container_name.clone()).await;
            return Err(ActionsError::Docker(e));
        }

        let mut last_exit_code: i32 = 0;

        for step in &job.steps {
            let result = self
                .execute_step(&container_name, step, &env_vars, secrets, &log_sink, 0)
                .await;
            let exit_code = match result {
                Ok(code) => code,
                Err(e) => {
                    log_sink(format!("[step error] {e}"));
                    cleanup(self.docker.clone(), container_name.clone()).await;
                    return Ok(JobResult {
                        status: JobStatus::Failure,
                        exit_code: 1,
                        artifacts: Vec::new(),
                    });
                }
            };

            last_exit_code = exit_code;

            if exit_code != 0 && !step.continue_on_error() {
                cleanup(self.docker.clone(), container_name.clone()).await;
                return Ok(JobResult {
                    status: JobStatus::Failure,
                    exit_code,
                    artifacts: Vec::new(),
                });
            }
        }

        // Collect any declared artifacts only when every step succeeded, and
        // before the container is torn down (the artifact's files only exist
        // inside the now-finished container).
        let mut artifacts = Vec::new();
        if last_exit_code == 0 {
            for step in &job.steps {
                let Some(uses) = &step.uses else { continue };
                if uses != "genome/upload-artifact" {
                    continue;
                }
                let Some(with) = &step.with else {
                    log_sink(
                        "[artifact] genome/upload-artifact step missing 'with: {name, path}'"
                            .to_string(),
                    );
                    continue;
                };
                let name = with.get("name").and_then(|v| v.as_str());
                let path = with.get("path").and_then(|v| v.as_str());
                let (Some(name), Some(path)) = (name, path) else {
                    log_sink(
                        "[artifact] genome/upload-artifact step requires both 'name' and 'path'"
                            .to_string(),
                    );
                    continue;
                };

                match self
                    .download_artifact(&container_name, run_id, name, path)
                    .await
                {
                    Ok(meta) => {
                        log_sink(format!(
                            "[artifact] collected '{}' ({} bytes)",
                            meta.name, meta.size_bytes
                        ));
                        artifacts.push(meta);
                    }
                    Err(e) => {
                        log_sink(format!("[artifact] failed to collect '{name}': {e}"));
                    }
                }
            }
        }

        cleanup(self.docker.clone(), container_name.clone()).await;

        Ok(JobResult {
            status: if last_exit_code == 0 {
                JobStatus::Success
            } else {
                JobStatus::Failure
            },
            exit_code: last_exit_code,
            artifacts,
        })
    }

    /// Execute a single top-level job step (`run:` or `uses:`), returning
    /// its exit code (0 = success).
    async fn execute_step(
        &self,
        container_name: &str,
        step: &Step,
        env_vars: &[String],
        secrets: &HashMap<String, String>,
        log_sink: &(impl Fn(String) + Send + Sync),
        depth: u8,
    ) -> Result<i32, ActionsError> {
        if let Some(uses) = &step.uses {
            if uses.starts_with("actions/checkout") {
                log_sink(format!("[step] uses: {uses} (no-op, repo already present)"));
                return Ok(0);
            }
            return self
                .execute_uses_step(container_name, uses, step.with.as_ref(), env_vars, secrets, log_sink, depth)
                .await;
        }

        let Some(run) = &step.run else { return Ok(0) };
        let step_name = step.name.clone().unwrap_or_else(|| run.clone());
        log_sink(format!("[step] {step_name}"));

        let shell = step.shell.clone().unwrap_or_else(|| "sh".to_string());
        let mut step_env: Vec<String> = env_vars.to_vec();
        if let Some(se) = &step.env {
            for (k, v) in se {
                step_env.push(format!("{k}={v}"));
            }
        }

        self.exec_run_in_container(container_name, &shell, run, secrets, step_env, log_sink)
            .await
    }

    /// Resolves and runs a `uses:` step that isn't `actions/checkout`: a
    /// direct `docker://image`, or a marketplace `owner/repo[/path]@ref`
    /// action whose `action.yml` declares `runs.using` as `docker`,
    /// `composite`, or a Node runtime. Anything else (unparseable, fetch
    /// failure, unsupported `runs.using`, or a composite action nested
    /// inside another composite action) is logged and treated as a no-op
    /// success (`Ok(0)`) rather than failing the job, matching the existing
    /// lenient "not supported, skipping" behavior for unrecognized actions.
    async fn execute_uses_step(
        &self,
        container_name: &str,
        uses: &str,
        with: Option<&HashMap<String, serde_yaml::Value>>,
        env_vars: &[String],
        secrets: &HashMap<String, String>,
        log_sink: &(impl Fn(String) + Send + Sync),
        depth: u8,
    ) -> Result<i32, ActionsError> {
        let Some(action_ref) = marketplace::ActionRef::parse(uses) else {
            log_sink(format!("action {uses} not supported in this runner, skipping"));
            return Ok(0);
        };

        match action_ref {
            marketplace::ActionRef::DockerImage(image) => {
                let resolved = marketplace::resolve_inputs(None, with);
                let mut env = env_vars.to_vec();
                env.extend(marketplace::input_env_vars(&resolved));
                log_sink(format!("[step] uses: {uses} (docker image)"));
                self.run_docker_action(container_name, image, None, None, env, log_sink).await
            }
            marketplace::ActionRef::Marketplace { owner, repo, path, git_ref } => {
                let fetched = {
                    let owner = owner.clone();
                    let repo = repo.clone();
                    let path = path.clone();
                    let git_ref = git_ref.clone();
                    tokio::task::spawn_blocking(move || {
                        marketplace::fetch_action(&owner, &repo, path.as_deref(), &git_ref)
                    })
                    .await
                    .map_err(|e| ActionsError::Marketplace(format!("fetch task panicked: {e}")))?
                };

                let fetched = match fetched {
                    Ok(f) => f,
                    Err(e) => {
                        log_sink(format!("action {uses} failed to fetch, skipping: {e}"));
                        return Ok(0);
                    }
                };

                let resolved = marketplace::resolve_inputs(Some(&fetched.metadata), with);

                match fetched.metadata.runs.using.as_str() {
                    "docker" => {
                        let Some(image_spec) = fetched.metadata.runs.image.clone() else {
                            log_sink(format!("action {uses} has runs.using=docker but no image, skipping"));
                            return Ok(0);
                        };
                        let image = if let Some(direct) = image_spec.strip_prefix("docker://") {
                            direct.to_string()
                        } else {
                            match self.build_action_image(&fetched.dir, &image_spec, log_sink).await {
                                Ok(tag) => tag,
                                Err(e) => {
                                    log_sink(format!("action {uses} failed to build image, skipping: {e}"));
                                    return Ok(0);
                                }
                            }
                        };
                        let entrypoint = fetched
                            .metadata
                            .runs
                            .entrypoint
                            .as_ref()
                            .map(|e| vec![marketplace::substitute_inputs(e, &resolved)]);
                        let cmd = fetched.metadata.runs.args.as_ref().map(|args| {
                            args.iter()
                                .map(|a| marketplace::substitute_inputs(a, &resolved))
                                .collect()
                        });
                        let mut env = env_vars.to_vec();
                        env.extend(marketplace::input_env_vars(&resolved));
                        log_sink(format!("[step] uses: {uses} (docker action)"));
                        self.run_docker_action(container_name, image, entrypoint, cmd, env, log_sink)
                            .await
                    }
                    "composite" => {
                        if depth > 0 {
                            log_sink(format!(
                                "action {uses} is a nested composite action, skipping (not supported)"
                            ));
                            return Ok(0);
                        }
                        let Some(steps) = fetched.metadata.runs.steps.clone() else {
                            log_sink(format!("action {uses} has runs.using=composite but no steps, skipping"));
                            return Ok(0);
                        };
                        log_sink(format!("[step] uses: {uses} (composite, {} steps)", steps.len()));
                        for nested in &steps {
                            let exit = Box::pin(self.execute_step(
                                container_name,
                                nested,
                                env_vars,
                                secrets,
                                log_sink,
                                depth + 1,
                            ))
                            .await?;
                            if exit != 0 && !nested.continue_on_error() {
                                return Ok(exit);
                            }
                        }
                        Ok(0)
                    }
                    using if using.starts_with("node") => {
                        let Some(main) = fetched.metadata.runs.main.clone() else {
                            log_sink(format!("action {uses} has runs.using={using} but no main, skipping"));
                            return Ok(0);
                        };
                        let image = marketplace::node_image_for_using(using).to_string();
                        let mut env = env_vars.to_vec();
                        env.extend(marketplace::input_env_vars(&resolved));
                        log_sink(format!("[step] uses: {uses} ({using} action)"));
                        self.run_node_action(container_name, &fetched.dir, &main, image, env, log_sink)
                            .await
                    }
                    other => {
                        log_sink(format!("action {uses} has unsupported runs.using: {other}, skipping"));
                        Ok(0)
                    }
                }
            }
        }
    }

    /// Runs a `run:` step's shell command via `docker exec` against the
    /// job's already-running container (secrets substituted into the
    /// command text first). Shared by top-level and composite-action steps.
    async fn exec_run_in_container(
        &self,
        container_name: &str,
        shell: &str,
        run: &str,
        secrets: &HashMap<String, String>,
        env: Vec<String>,
        log_sink: &(impl Fn(String) + Send + Sync),
    ) -> Result<i32, ActionsError> {
        let run = crate::secrets::substitute_secrets(run, secrets);

        let exec = self
            .docker
            .create_exec(
                container_name,
                CreateExecOptions {
                    cmd: Some(vec![shell.to_string(), "-c".to_string(), run]),
                    attach_stdout: Some(true),
                    attach_stderr: Some(true),
                    env: Some(env),
                    working_dir: Some("/workspace".to_string()),
                    ..Default::default()
                },
            )
            .await
            .map_err(ActionsError::Docker)?;

        match self.docker.start_exec(&exec.id, None).await.map_err(ActionsError::Docker)? {
            StartExecResults::Attached { mut output, .. } => {
                while let Some(chunk) = output.next().await {
                    match chunk {
                        Ok(msg) => {
                            for line in msg.to_string().lines() {
                                log_sink(line.to_string());
                            }
                        }
                        Err(e) => {
                            log_sink(format!("[error reading output] {e}"));
                            break;
                        }
                    }
                }
            }
            StartExecResults::Detached => {}
        }

        let inspect = self.docker.inspect_exec(&exec.id).await.map_err(ActionsError::Docker)?;
        Ok(inspect.exit_code.unwrap_or(0) as i32)
    }

    /// Downloads `/workspace` out of the job container as a tar stream.
    async fn copy_workspace_out(&self, container_name: &str) -> Result<Vec<u8>, ActionsError> {
        let mut stream = self.docker.download_from_container(
            container_name,
            Some(DownloadFromContainerOptions { path: "/workspace".to_string() }),
        );
        let mut bytes = Vec::new();
        while let Some(chunk) = stream.next().await {
            bytes.extend_from_slice(&chunk.map_err(ActionsError::Docker)?);
        }
        Ok(bytes)
    }

    /// Uploads a tar produced by `copy_workspace_out` (whose entries are
    /// rooted at `workspace/...`) into `container_name` at `/`, so it lands
    /// at `/workspace` there too.
    async fn copy_workspace_in(&self, container_name: &str, tar_bytes: Vec<u8>) -> Result<(), ActionsError> {
        self.docker
            .upload_to_container(
                container_name,
                Some(UploadToContainerOptions { path: "/", no_overwrite_dir_non_dir: "false" }),
                tar_bytes.into(),
            )
            .await
            .map_err(ActionsError::Docker)
    }

    /// Runs a Docker container action (or a direct `docker://image` step)
    /// as its own short-lived sibling container: copies the job's
    /// `/workspace` in, runs the image's entrypoint/cmd to completion,
    /// copies `/workspace` back out to the job container (so files the
    /// action wrote/changed are visible to later steps), then removes the
    /// side container. Real GitHub Actions runners give Docker actions the
    /// path `/github/workspace`; using `/workspace` here instead is a
    /// deliberate simplification (see module docs) that avoids needing to
    /// create `/github` in an arbitrary base image before the copy-in.
    #[allow(clippy::too_many_arguments)]
    async fn run_docker_action(
        &self,
        job_container_name: &str,
        image: String,
        entrypoint: Option<Vec<String>>,
        cmd: Option<Vec<String>>,
        env: Vec<String>,
        log_sink: &(impl Fn(String) + Send + Sync),
    ) -> Result<i32, ActionsError> {
        {
            let mut stream = self.docker.create_image(
                Some(CreateImageOptions { from_image: image.clone(), ..Default::default() }),
                None,
                None,
            );
            while let Some(item) = stream.next().await {
                if let Err(e) = item {
                    tracing::warn!("image pull warning for {image}: {e}");
                    break;
                }
            }
        }

        let side_name = format!("genome-action-{}", Uuid::new_v4());
        let config = Config {
            image: Some(image),
            entrypoint,
            cmd,
            env: Some(env),
            working_dir: Some("/workspace".to_string()),
            ..Default::default()
        };

        self.docker
            .create_container(Some(CreateContainerOptions { name: side_name.clone(), platform: None }), config)
            .await
            .map_err(ActionsError::Docker)?;

        let cleanup_side = || {
            let docker = self.docker.clone();
            let name = side_name.clone();
            async move {
                let _ = docker
                    .remove_container(&name, Some(RemoveContainerOptions { force: true, ..Default::default() }))
                    .await;
            }
        };

        let workspace_tar = match self.copy_workspace_out(job_container_name).await {
            Ok(t) => t,
            Err(e) => {
                cleanup_side().await;
                return Err(e);
            }
        };
        if let Err(e) = self.copy_workspace_in(&side_name, workspace_tar).await {
            cleanup_side().await;
            return Err(e);
        }

        if let Err(e) = self.docker.start_container::<String>(&side_name, None).await {
            cleanup_side().await;
            return Err(ActionsError::Docker(e));
        }

        let exit_code = {
            let mut waits = self
                .docker
                .wait_container(&side_name, Some(WaitContainerOptions { condition: "not-running".to_string() }));
            match waits.next().await {
                Some(Ok(resp)) => resp.status_code as i32,
                Some(Err(e)) => {
                    cleanup_side().await;
                    return Err(ActionsError::Docker(e));
                }
                None => 0,
            }
        };

        let mut logs = self.docker.logs(
            &side_name,
            Some(LogsOptions::<String> {
                follow: false,
                stdout: true,
                stderr: true,
                tail: "all".to_string(),
                ..Default::default()
            }),
        );
        while let Some(chunk) = logs.next().await {
            match chunk {
                Ok(msg) => {
                    for line in msg.to_string().lines() {
                        log_sink(line.to_string());
                    }
                }
                Err(e) => {
                    log_sink(format!("[error reading output] {e}"));
                    break;
                }
            }
        }

        // Best-effort: propagate any workspace changes the action made
        // back into the job's own container for subsequent steps to see.
        if let Ok(tar) = self.copy_workspace_out(&side_name).await {
            let _ = self.copy_workspace_in(job_container_name, tar).await;
        }

        cleanup_side().await;
        Ok(exit_code)
    }

    /// Builds a Docker image from a `Dockerfile` living inside a fetched
    /// action's directory (the `runs.image` value when it isn't a
    /// `docker://...` reference), tagging it uniquely for this run.
    async fn build_action_image(
        &self,
        action_dir: &Path,
        dockerfile_relative: &str,
        log_sink: &(impl Fn(String) + Send + Sync),
    ) -> Result<String, ActionsError> {
        let action_dir = action_dir.to_path_buf();
        let dockerfile_relative = dockerfile_relative.to_string();
        let tar_bytes = tokio::task::spawn_blocking(move || -> Result<Vec<u8>, ActionsError> {
            let mut builder = tar::Builder::new(Vec::new());
            builder
                .append_dir_all(".", &action_dir)
                .map_err(|e| ActionsError::Marketplace(e.to_string()))?;
            builder.into_inner().map_err(|e| ActionsError::Marketplace(e.to_string()))
        })
        .await
        .map_err(|e| ActionsError::Marketplace(format!("build context task panicked: {e}")))??;

        let tag = format!("genome-action-image:{}", Uuid::new_v4());
        let mut stream = self.docker.build_image(
            BuildImageOptions {
                dockerfile: dockerfile_relative,
                t: tag.clone(),
                rm: true,
                ..Default::default()
            },
            None,
            Some(tar_bytes.into()),
        );
        while let Some(item) = stream.next().await {
            match item {
                Ok(msg) => {
                    if let Some(stream_text) = msg.stream {
                        for line in stream_text.lines() {
                            log_sink(format!("[build] {line}"));
                        }
                    }
                }
                Err(e) => return Err(ActionsError::Docker(e)),
            }
        }
        Ok(tag)
    }

    /// Runs a JS action (`runs.using: node12/16/18/20`) inside a helper
    /// Node image matching the requested runtime, since the job's own
    /// `runs-on` image has no reason to include Node. Copies both the
    /// job's `/workspace` and the action's own source (containing `main`)
    /// into the helper container, runs `node <main>`, then propagates
    /// `/workspace` changes back exactly like `run_docker_action`.
    #[allow(clippy::too_many_arguments)]
    async fn run_node_action(
        &self,
        job_container_name: &str,
        action_dir: &Path,
        main: &str,
        image: String,
        env: Vec<String>,
        log_sink: &(impl Fn(String) + Send + Sync),
    ) -> Result<i32, ActionsError> {
        {
            let mut stream = self.docker.create_image(
                Some(CreateImageOptions { from_image: image.clone(), ..Default::default() }),
                None,
                None,
            );
            while let Some(item) = stream.next().await {
                if let Err(e) = item {
                    tracing::warn!("image pull warning for {image}: {e}");
                    break;
                }
            }
        }

        let side_name = format!("genome-action-{}", Uuid::new_v4());
        let config = Config {
            image: Some(image),
            cmd: Some(vec![
                "sh".to_string(),
                "-c".to_string(),
                "mkdir -p /workspace /action && tail -f /dev/null".to_string(),
            ]),
            env: Some(env),
            working_dir: Some("/workspace".to_string()),
            ..Default::default()
        };

        self.docker
            .create_container(Some(CreateContainerOptions { name: side_name.clone(), platform: None }), config)
            .await
            .map_err(ActionsError::Docker)?;

        let cleanup_side = || {
            let docker = self.docker.clone();
            let name = side_name.clone();
            async move {
                let _ = docker
                    .remove_container(&name, Some(RemoveContainerOptions { force: true, ..Default::default() }))
                    .await;
            }
        };

        if let Err(e) = self.docker.start_container::<String>(&side_name, None).await {
            cleanup_side().await;
            return Err(ActionsError::Docker(e));
        }

        let workspace_tar = match self.copy_workspace_out(job_container_name).await {
            Ok(t) => t,
            Err(e) => {
                cleanup_side().await;
                return Err(e);
            }
        };
        if let Err(e) = self.copy_workspace_in(&side_name, workspace_tar).await {
            cleanup_side().await;
            return Err(e);
        }

        let action_dir = action_dir.to_path_buf();
        let action_tar = match tokio::task::spawn_blocking(move || -> Result<Vec<u8>, ActionsError> {
            let mut builder = tar::Builder::new(Vec::new());
            builder
                .append_dir_all("action", &action_dir)
                .map_err(|e| ActionsError::Marketplace(e.to_string()))?;
            builder.into_inner().map_err(|e| ActionsError::Marketplace(e.to_string()))
        })
        .await
        {
            Ok(Ok(t)) => t,
            Ok(Err(_)) | Err(_) => {
                cleanup_side().await;
                return Err(ActionsError::Marketplace("failed to package action source".to_string()));
            }
        };
        if let Err(e) = self
            .docker
            .upload_to_container(
                &side_name,
                Some(UploadToContainerOptions { path: "/", no_overwrite_dir_non_dir: "false" }),
                action_tar.into(),
            )
            .await
        {
            cleanup_side().await;
            return Err(ActionsError::Docker(e));
        }

        let exec = match self
            .docker
            .create_exec(
                &side_name,
                CreateExecOptions {
                    cmd: Some(vec!["node".to_string(), format!("/action/{main}")]),
                    attach_stdout: Some(true),
                    attach_stderr: Some(true),
                    working_dir: Some("/workspace".to_string()),
                    ..Default::default()
                },
            )
            .await
        {
            Ok(e) => e,
            Err(e) => {
                cleanup_side().await;
                return Err(ActionsError::Docker(e));
            }
        };

        match self.docker.start_exec(&exec.id, None).await {
            Ok(StartExecResults::Attached { mut output, .. }) => {
                while let Some(chunk) = output.next().await {
                    match chunk {
                        Ok(msg) => {
                            for line in msg.to_string().lines() {
                                log_sink(line.to_string());
                            }
                        }
                        Err(e) => {
                            log_sink(format!("[error reading output] {e}"));
                            break;
                        }
                    }
                }
            }
            Ok(StartExecResults::Detached) => {}
            Err(e) => {
                cleanup_side().await;
                return Err(ActionsError::Docker(e));
            }
        }

        let exit_code = match self.docker.inspect_exec(&exec.id).await {
            Ok(inspect) => inspect.exit_code.unwrap_or(0) as i32,
            Err(e) => {
                cleanup_side().await;
                return Err(ActionsError::Docker(e));
            }
        };

        if let Ok(tar) = self.copy_workspace_out(&side_name).await {
            let _ = self.copy_workspace_in(job_container_name, tar).await;
        }

        cleanup_side().await;
        Ok(exit_code)
    }

    /// Downloads `container_path` out of the (still-running) container as a
    /// tar stream and writes it verbatim to
    /// `{artifacts_root}/{run_id}/{name}.tar`, returning its metadata.
    async fn download_artifact(
        &self,
        container_name: &str,
        run_id: Uuid,
        name: &str,
        container_path: &str,
    ) -> Result<ArtifactMeta, ActionsError> {
        let mut stream = self.docker.download_from_container(
            container_name,
            Some(DownloadFromContainerOptions {
                path: container_path.to_string(),
            }),
        );

        let mut bytes: Vec<u8> = Vec::new();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(ActionsError::Docker)?;
            bytes.extend_from_slice(&chunk);
        }

        let run_dir = self.artifacts_root.join(run_id.to_string());
        tokio::fs::create_dir_all(&run_dir)
            .await
            .map_err(|e| ActionsError::Artifact(e.to_string()))?;

        let file_path = run_dir.join(format!("{name}.tar"));
        tokio::fs::write(&file_path, &bytes)
            .await
            .map_err(|e| ActionsError::Artifact(e.to_string()))?;

        Ok(ArtifactMeta {
            name: name.to_string(),
            file_path: file_path.to_string_lossy().to_string(),
            size_bytes: bytes.len() as i64,
        })
    }
}

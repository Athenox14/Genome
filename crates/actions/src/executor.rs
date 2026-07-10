//! Docker-based execution of a single workflow job via `bollard`.

use std::collections::HashMap;

use bollard::container::{
    Config, CreateContainerOptions, RemoveContainerOptions, UploadToContainerOptions,
};
use bollard::exec::{CreateExecOptions, StartExecResults};
use bollard::image::CreateImageOptions;
use bollard::Docker;
use futures_util::StreamExt;

use crate::workflow::Job;
use crate::ActionsError;

/// Final status of a job run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobStatus {
    Success,
    Failure,
}

/// Result of running a job to completion.
#[derive(Debug, Clone)]
pub struct JobResult {
    pub status: JobStatus,
    pub exit_code: i32,
}

/// Executes workflow jobs inside disposable Docker containers.
pub struct Executor {
    docker: Docker,
}

impl Executor {
    /// Connect to the local Docker daemon using platform defaults
    /// (named pipe on Windows, unix socket elsewhere).
    pub fn new() -> Result<Executor, ActionsError> {
        let docker =
            Docker::connect_with_local_defaults().map_err(ActionsError::Docker)?;
        Ok(Executor { docker })
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
        job: &Job,
        workdir_repo_archive: &[u8],
        env_extra: HashMap<String, String>,
        secrets: &HashMap<String, String>,
        log_sink: impl Fn(String) + Send + 'static,
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

        // Upload the repo archive into /workspace.
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
            if let Some(uses) = &step.uses {
                if uses.starts_with("actions/checkout") {
                    log_sink(format!("[step] uses: {uses} (no-op, repo already present)"));
                    continue;
                }
                log_sink(format!(
                    "action {uses} not supported in this runner, skipping"
                ));
                continue;
            }

            let Some(run) = &step.run else {
                // Neither `run` nor `uses` set: nothing to do.
                continue;
            };

            let step_name = step.name.clone().unwrap_or_else(|| run.clone());
            log_sink(format!("[step] {step_name}"));

            let run = crate::secrets::substitute_secrets(run, secrets);

            let shell = step.shell.clone().unwrap_or_else(|| "sh".to_string());
            let mut step_env: Vec<String> = env_vars.clone();
            if let Some(se) = &step.env {
                for (k, v) in se {
                    step_env.push(format!("{k}={v}"));
                }
            }

            let exec = self
                .docker
                .create_exec(
                    &container_name,
                    CreateExecOptions {
                        cmd: Some(vec![shell, "-c".to_string(), run.clone()]),
                        attach_stdout: Some(true),
                        attach_stderr: Some(true),
                        env: Some(step_env),
                        working_dir: Some("/workspace".to_string()),
                        ..Default::default()
                    },
                )
                .await;

            let exec = match exec {
                Ok(e) => e,
                Err(e) => {
                    cleanup(self.docker.clone(), container_name.clone()).await;
                    return Err(ActionsError::Docker(e));
                }
            };

            match self.docker.start_exec(&exec.id, None).await {
                Ok(StartExecResults::Attached { mut output, .. }) => {
                    while let Some(chunk) = output.next().await {
                        match chunk {
                            Ok(msg) => {
                                let text = msg.to_string();
                                for line in text.lines() {
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
                    cleanup(self.docker.clone(), container_name.clone()).await;
                    return Err(ActionsError::Docker(e));
                }
            }

            let exit_code = match self.docker.inspect_exec(&exec.id).await {
                Ok(inspect) => inspect.exit_code.unwrap_or(0) as i32,
                Err(e) => {
                    cleanup(self.docker.clone(), container_name.clone()).await;
                    return Err(ActionsError::Docker(e));
                }
            };

            last_exit_code = exit_code;

            if exit_code != 0 && !step.continue_on_error() {
                cleanup(self.docker.clone(), container_name.clone()).await;
                return Ok(JobResult {
                    status: JobStatus::Failure,
                    exit_code,
                });
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
        })
    }
}

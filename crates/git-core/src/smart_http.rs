use std::path::Path;
use std::process::Stdio;

use bytes::Bytes;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;

use crate::error::{GitCoreError, Result};

/// The two git smart-HTTP services we support.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitService {
    UploadPack,
    ReceivePack,
}

impl GitService {
    /// Parse the `service` query parameter (e.g. `git-upload-pack`).
    pub fn parse(service: &str) -> Result<Self> {
        match service {
            "git-upload-pack" => Ok(GitService::UploadPack),
            "git-receive-pack" => Ok(GitService::ReceivePack),
            other => Err(GitCoreError::UnsupportedService(other.to_string())),
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            GitService::UploadPack => "git-upload-pack",
            GitService::ReceivePack => "git-receive-pack",
        }
    }

    fn binary_name(&self) -> &'static str {
        match self {
            GitService::UploadPack => "git-upload-pack",
            GitService::ReceivePack => "git-receive-pack",
        }
    }
}

/// Encode a single pkt-line: a 4-hex-digit length prefix followed by the
/// payload. The length includes the 4-byte prefix itself.
fn pkt_line(payload: &[u8]) -> Vec<u8> {
    let len = payload.len() + 4;
    let mut out = format!("{len:04x}").into_bytes();
    out.extend_from_slice(payload);
    out
}

/// The pkt-line "flush" marker.
fn flush_pkt() -> Vec<u8> {
    b"0000".to_vec()
}

/// Handle a `GET /{owner}/{repo}.git/info/refs?service=git-upload-pack` (or
/// receive-pack) request: runs `git {service} --stateless-rpc --advertise-refs`
/// against the bare repo and wraps the output in the smart-HTTP pkt-line
/// service announcement framing.
pub async fn handle_info_refs(repo_path: &Path, service: GitService) -> Result<Vec<u8>> {
    let output = Command::new(service.binary_name())
        .arg("--stateless-rpc")
        .arg("--advertise-refs")
        .arg(repo_path)
        .output()
        .await?;

    if !output.status.success() {
        return Err(GitCoreError::Subprocess(
            service.binary_name().to_string(),
            String::from_utf8_lossy(&output.stderr).to_string(),
        ));
    }

    let mut body = Vec::new();
    body.extend_from_slice(&pkt_line(
        format!("# service={}\n", service.as_str()).as_bytes(),
    ));
    body.extend_from_slice(&flush_pkt());
    body.extend_from_slice(&output.stdout);
    Ok(body)
}

/// Handle a `POST /{owner}/{repo}.git/{service}` request: pipes `body` into
/// `git {service} --stateless-rpc` and returns its raw stdout, which is
/// already correctly framed by git itself.
pub async fn handle_service_rpc(
    repo_path: &Path,
    service: GitService,
    body: Bytes,
) -> Result<Vec<u8>> {
    let mut child = Command::new(service.binary_name())
        .arg("--stateless-rpc")
        .arg(repo_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(&body).await?;
        // Drop to close stdin so the subprocess sees EOF.
    }

    let mut stdout = Vec::new();
    if let Some(mut out) = child.stdout.take() {
        out.read_to_end(&mut stdout).await?;
    }

    let mut stderr = Vec::new();
    if let Some(mut err) = child.stderr.take() {
        err.read_to_end(&mut stderr).await?;
    }

    let status = child.wait().await?;
    if !status.success() {
        return Err(GitCoreError::Subprocess(
            service.binary_name().to_string(),
            String::from_utf8_lossy(&stderr).to_string(),
        ));
    }

    Ok(stdout)
}

/// The correct `Content-Type` header value for an info/refs response.
pub fn info_refs_content_type(service: GitService) -> String {
    format!("application/x-{}-advertisement", service.as_str())
}

/// The correct `Content-Type` header value for a service RPC response.
pub fn service_rpc_content_type(service: GitService) -> String {
    format!("application/x-{}-result", service.as_str())
}

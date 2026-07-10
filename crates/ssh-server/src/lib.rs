//! `ssh-server`: a minimal SSH server exposing the git-over-SSH protocol
//! (`git-upload-pack`, `git-receive-pack`, `git-upload-archive`) against the
//! same on-disk bare repositories the smart-HTTP transport uses.
//!
//! Authentication is public-key only: the client's offered key is
//! fingerprinted (same SHA256 fingerprint scheme used when storing keys via
//! the GraphQL API) and looked up against `ssh_keys` rows. Unknown keys are
//! rejected. Once authenticated, `exec` requests are parsed for the three
//! supported git SSH commands, permission-checked against the repository via
//! the same effective-permission rules used by the HTTP path, and if
//! authorized, piped directly to the corresponding `git-*` subprocess.

pub mod error;
pub mod permission;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;

use russh::keys::{Algorithm, PrivateKey, PublicKey};
use russh::server::{Auth, Config as RusshConfig, Handler, Msg, Server as RusshServer, Session};
use russh::{Channel, ChannelId};
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::{ChildStdin, Command};
use uuid::Uuid;

use error::{Result, SshServerError};

/// Runtime configuration for the SSH server.
#[derive(Debug, Clone)]
pub struct SshServerConfig {
    /// Address to listen on, e.g. `0.0.0.0:2222`.
    ///
    /// Port 2222 is the default because binding port 22 usually requires
    /// root/administrator privileges; deployments that want the standard
    /// port should either run this process with elevated privileges or
    /// forward port 22 to this listener at the network layer.
    pub listen_addr: String,
    /// Path to the persisted host key (OpenSSH PEM format). Generated on
    /// first run if it does not already exist.
    pub host_key_path: PathBuf,
}

/// State shared across all SSH connections.
pub struct SharedState {
    pub db: DatabaseConnection,
    pub repo_manager: Arc<git_core::RepoManager>,
}

/// Load the host key from `path`, generating and persisting a new Ed25519
/// key on first run.
pub fn load_or_generate_host_key(path: &Path) -> Result<PrivateKey> {
    if path.exists() {
        PrivateKey::read_openssh_file(path).map_err(|e| {
            SshServerError::Other(format!(
                "failed to read SSH host key at {}: {e}",
                path.display()
            ))
        })
    } else {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut rng = rand::rng();
        let key = PrivateKey::random(&mut rng, Algorithm::Ed25519).map_err(|e| {
            SshServerError::Other(format!("failed to generate SSH host key: {e}"))
        })?;
        key.write_openssh_file(path, russh::keys::ssh_key::LineEnding::LF)
            .map_err(|e| {
                SshServerError::Other(format!(
                    "failed to write SSH host key at {}: {e}",
                    path.display()
                ))
            })?;
        tracing::info!("generated new SSH host key at {}", path.display());
        Ok(key)
    }
}

/// Run the SSH server until the process is terminated. Intended to be
/// spawned as a background tokio task alongside the HTTP server.
pub async fn run(config: SshServerConfig, state: Arc<SharedState>) -> anyhow::Result<()> {
    let host_key = load_or_generate_host_key(&config.host_key_path)?;

    let russh_config = Arc::new(RusshConfig {
        keys: vec![host_key],
        ..Default::default()
    });

    let mut server = GitSshServer { state };
    tracing::info!("SSH server listening on {}", config.listen_addr);
    server
        .run_on_address(russh_config, config.listen_addr.clone())
        .await?;
    Ok(())
}

struct GitSshServer {
    state: Arc<SharedState>,
}

impl RusshServer for GitSshServer {
    type Handler = GitSshHandler;

    fn new_client(&mut self, _peer_addr: Option<std::net::SocketAddr>) -> Self::Handler {
        GitSshHandler {
            state: self.state.clone(),
            user_id: None,
            username: None,
            child_stdins: HashMap::new(),
        }
    }

    fn handle_session_error(&mut self, error: <Self::Handler as Handler>::Error) {
        tracing::warn!("SSH session error: {error}");
    }
}

/// The three git-over-SSH commands we support.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GitSshService {
    UploadPack,
    ReceivePack,
    UploadArchive,
}

impl GitSshService {
    fn binary_name(&self) -> &'static str {
        match self {
            GitSshService::UploadPack => "git-upload-pack",
            GitSshService::ReceivePack => "git-receive-pack",
            GitSshService::UploadArchive => "git-upload-archive",
        }
    }

    /// Whether this service requires write access (as opposed to read).
    fn requires_write(&self) -> bool {
        matches!(self, GitSshService::ReceivePack)
    }
}

/// Parse an `exec` command string such as
/// `git-upload-pack '/owner/repo.git'` into a service plus owner/repo slugs.
fn parse_git_command(command: &str) -> Option<(GitSshService, String, String)> {
    let command = command.trim();
    let (verb, rest) = command.split_once(char::is_whitespace)?;

    let service = match verb {
        "git-upload-pack" => GitSshService::UploadPack,
        "git-receive-pack" => GitSshService::ReceivePack,
        "git-upload-archive" => GitSshService::UploadArchive,
        _ => return None,
    };

    let rest = rest.trim();
    // Path argument may be single- or double-quoted, or bare.
    let path = if (rest.starts_with('\'') && rest.ends_with('\'') && rest.len() >= 2)
        || (rest.starts_with('"') && rest.ends_with('"') && rest.len() >= 2)
    {
        &rest[1..rest.len() - 1]
    } else {
        rest
    };

    let trimmed = path.trim_start_matches('/');
    let trimmed = trimmed.strip_suffix(".git").unwrap_or(trimmed);
    let mut parts = trimmed.splitn(2, '/');
    let owner = parts.next()?.to_string();
    let repo = parts.next()?.to_string();
    if owner.is_empty() || repo.is_empty() {
        return None;
    }
    Some((service, owner, repo))
}

struct GitSshHandler {
    state: Arc<SharedState>,
    user_id: Option<Uuid>,
    username: Option<String>,
    child_stdins: HashMap<ChannelId, ChildStdin>,
}

impl GitSshHandler {
    /// Reject unless a repo was found and the authenticated user has at
    /// least the permission the requested service needs.
    async fn authorize(
        &self,
        owner: &str,
        repo_name: &str,
        service: GitSshService,
    ) -> anyhow::Result<Option<entity::repository::Model>> {
        let Some(user_id) = self.user_id else {
            return Ok(None);
        };

        let Some(repo) = permission::find_repo(&self.state.db, owner, repo_name).await? else {
            return Ok(None);
        };

        let perm = permission::repo_permission(&self.state.db, &repo, user_id).await?;

        let authorized = match perm {
            Some(p) => {
                if service.requires_write() {
                    p >= auth::Permission::Write
                } else {
                    true
                }
            }
            None => false,
        };

        Ok(if authorized { Some(repo) } else { None })
    }
}

impl Handler for GitSshHandler {
    type Error = SshServerError;

    async fn auth_publickey(
        &mut self,
        user: &str,
        public_key: &PublicKey,
    ) -> std::result::Result<Auth, Self::Error> {
        let key_line = match public_key.to_openssh() {
            Ok(line) => line,
            Err(_) => return Ok(Auth::reject()),
        };

        let fingerprint = match auth::ssh_key_fingerprint(&key_line) {
            Ok(fp) => fp,
            Err(_) => return Ok(Auth::reject()),
        };

        let key_row = entity::prelude::SshKey::find()
            .filter(entity::ssh_key::Column::Fingerprint.eq(fingerprint))
            .one(&self.state.db)
            .await?;

        let Some(key_row) = key_row else {
            return Ok(Auth::reject());
        };

        let user_row = entity::prelude::User::find_by_id(key_row.user_id)
            .one(&self.state.db)
            .await?;

        let Some(user_row) = user_row else {
            return Ok(Auth::reject());
        };

        tracing::info!(
            "SSH public-key auth succeeded for user '{}' (ssh user field: '{}')",
            user_row.username,
            user
        );
        self.user_id = Some(user_row.id);
        self.username = Some(user_row.username);
        Ok(Auth::Accept)
    }

    async fn exec_request(
        &mut self,
        channel: ChannelId,
        data: &[u8],
        session: &mut Session,
    ) -> std::result::Result<(), Self::Error> {
        let command = String::from_utf8_lossy(data).to_string();

        let Some((service, owner, repo_name)) = parse_git_command(&command) else {
            tracing::warn!("rejecting unsupported SSH exec command: {command:?}");
            let _ = session.channel_failure(channel);
            let _ = session.close(channel);
            return Ok(());
        };

        if git_core::validate_slug(&owner).is_err() || git_core::validate_slug(&repo_name).is_err()
        {
            let _ = session.channel_failure(channel);
            let _ = session.close(channel);
            return Ok(());
        }

        let authorized = self
            .authorize(&owner, &repo_name, service)
            .await
            .map_err(|e| SshServerError::Other(e.to_string()))?;

        let Some(_repo) = authorized else {
            tracing::warn!(
                "SSH user {:?} denied {} on {owner}/{repo_name}",
                self.username,
                service.binary_name()
            );
            let _ = session.channel_failure(channel);
            let _ = session.close(channel);
            return Ok(());
        };

        let repo_path = self
            .state
            .repo_manager
            .repo_path(&owner, &repo_name)
            .map_err(SshServerError::GitCore)?;

        let mut child = Command::new(service.binary_name())
            .arg(&repo_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        let stdin = child.stdin.take();
        let mut stdout = child.stdout.take();
        let mut stderr = child.stderr.take();

        if let Some(stdin) = stdin {
            self.child_stdins.insert(channel, stdin);
        }

        let handle = session.handle();

        tokio::spawn(async move {
            let mut stdout_buf = [0u8; 32 * 1024];
            let mut stderr_buf = [0u8; 32 * 1024];

            loop {
                tokio::select! {
                    n = async {
                        match stdout.as_mut() {
                            Some(s) => s.read(&mut stdout_buf).await,
                            None => std::future::pending().await,
                        }
                    } => {
                        match n {
                            Ok(0) => { stdout = None; }
                            Ok(n) => {
                                if handle.data(channel, bytes::Bytes::copy_from_slice(&stdout_buf[..n])).await.is_err() {
                                    break;
                                }
                            }
                            Err(_) => { stdout = None; }
                        }
                    }
                    n = async {
                        match stderr.as_mut() {
                            Some(s) => s.read(&mut stderr_buf).await,
                            None => std::future::pending().await,
                        }
                    } => {
                        match n {
                            Ok(0) => { stderr = None; }
                            Ok(n) => {
                                if handle
                                    .extended_data(channel, 1, bytes::Bytes::copy_from_slice(&stderr_buf[..n]))
                                    .await
                                    .is_err()
                                {
                                    break;
                                }
                            }
                            Err(_) => { stderr = None; }
                        }
                    }
                    else => break,
                }

                if stdout.is_none() && stderr.is_none() {
                    break;
                }
            }

            let status = child.wait().await;
            let code = status.ok().and_then(|s| s.code()).unwrap_or(1) as u32;
            let _ = handle.exit_status_request(channel, code).await;
            let _ = handle.eof(channel).await;
            let _ = handle.close(channel).await;
        });

        session.channel_success(channel)?;
        Ok(())
    }

    async fn data(
        &mut self,
        channel: ChannelId,
        data: &[u8],
        _session: &mut Session,
    ) -> std::result::Result<(), Self::Error> {
        if let Some(stdin) = self.child_stdins.get_mut(&channel) {
            if stdin.write_all(data).await.is_err() {
                self.child_stdins.remove(&channel);
            }
        }
        Ok(())
    }

    async fn channel_eof(
        &mut self,
        channel: ChannelId,
        _session: &mut Session,
    ) -> std::result::Result<(), Self::Error> {
        // Dropping the ChildStdin closes the write half, so the subprocess
        // sees EOF on its stdin, matching the smart-HTTP behavior of
        // closing stdin once the request body has been fully written.
        self.child_stdins.remove(&channel);
        Ok(())
    }

    async fn channel_close(
        &mut self,
        channel: ChannelId,
        _session: &mut Session,
    ) -> std::result::Result<(), Self::Error> {
        self.child_stdins.remove(&channel);
        Ok(())
    }

    async fn channel_open_session(
        &mut self,
        _channel: Channel<Msg>,
        reply: russh::server::ChannelOpenHandle,
        _session: &mut Session,
    ) -> std::result::Result<(), Self::Error> {
        reply.accept().await;
        Ok(())
    }
}

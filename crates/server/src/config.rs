use anyhow::{Context, Result};

/// Runtime configuration for the server, loaded from environment variables.
#[derive(Debug, Clone)]
pub struct Config {
    /// Directory Hiqlite (the embedded, Raft-replicated SQLite database)
    /// stores its data, WAL and snapshots under. Replaces the old
    /// `DATABASE_URL` Postgres connection string entirely -- there is no
    /// external database process to point at any more.
    pub data_dir: String,
    /// Address the embedded Hiqlite node's API listens on / is reached at
    /// (used both for the single-node `NodeConfig` built at startup and by
    /// the `check-push-protection` CLI subcommand, which connects to the
    /// already-running server as a remote Hiqlite client rather than
    /// starting its own Raft node).
    pub hiqlite_api_addr: String,
    /// Address the embedded Hiqlite node's internal Raft traffic listens on.
    pub hiqlite_raft_addr: String,
    pub jwt_secret: String,
    pub repos_root_path: String,
    pub listen_addr: String,
    pub docker_socket_path: String,
    /// 32 raw bytes, base64-encoded, used to encrypt/decrypt Actions
    /// secrets at rest (AES-256-GCM). Optional: features that need it
    /// (setting/using repo secrets) will error out if it's unset.
    pub secrets_encryption_key: Option<String>,
    /// Address the git-over-SSH server listens on.
    ///
    /// Defaults to port 2222 rather than the standard port 22, since
    /// binding 22 usually requires root/administrator privileges. To serve
    /// on 22 either run this process with elevated privileges or forward
    /// port 22 to this listener at the network layer.
    pub ssh_listen_addr: String,
    /// Path to the persisted SSH host key (generated on first run).
    pub ssh_host_key_path: String,
    /// Base URL of the frontend app, used to build the `/login?then=...`
    /// redirect for unauthenticated `GET /oauth/authorize` requests.
    pub frontend_url: String,
    /// Root directory under which uploaded packages are stored, as
    /// `{packages_root_path}/{owner}/{name}/{version}/{filename}`. Defaults
    /// to a `packages` directory alongside `repos_root_path`.
    pub packages_root_path: String,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        let data_dir = std::env::var("DATA_DIR").unwrap_or_else(|_| "./data/hiqlite".to_string());
        let hiqlite_api_addr =
            std::env::var("HIQLITE_API_ADDR").unwrap_or_else(|_| "127.0.0.1:8200".to_string());
        let hiqlite_raft_addr =
            std::env::var("HIQLITE_RAFT_ADDR").unwrap_or_else(|_| "127.0.0.1:8100".to_string());
        let jwt_secret = std::env::var("JWT_SECRET")
            .context("JWT_SECRET environment variable must be set")?;
        let repos_root_path =
            std::env::var("REPOS_ROOT_PATH").unwrap_or_else(|_| "./repos".to_string());
        let listen_addr =
            std::env::var("LISTEN_ADDR").unwrap_or_else(|_| "0.0.0.0:8000".to_string());

        let default_docker_socket = if cfg!(windows) {
            "npipe:////./pipe/docker_engine".to_string()
        } else {
            "unix:///var/run/docker.sock".to_string()
        };
        let docker_socket_path =
            std::env::var("DOCKER_SOCKET_PATH").unwrap_or(default_docker_socket);

        let secrets_encryption_key = std::env::var("SECRETS_ENCRYPTION_KEY").ok();

        let ssh_listen_addr =
            std::env::var("SSH_LISTEN_ADDR").unwrap_or_else(|_| "0.0.0.0:2222".to_string());
        let ssh_host_key_path = std::env::var("SSH_HOST_KEY_PATH")
            .unwrap_or_else(|_| "./data/ssh_host_key".to_string());

        let frontend_url =
            std::env::var("FRONTEND_URL").unwrap_or_else(|_| "http://localhost:3000".to_string());
        let packages_root_path = std::env::var("PACKAGES_ROOT_PATH").unwrap_or_else(|_| {
            std::path::Path::new(&repos_root_path)
                .parent()
                .unwrap_or_else(|| std::path::Path::new("."))
                .join("packages")
                .to_string_lossy()
                .to_string()
        });

        Ok(Config {
            data_dir,
            hiqlite_api_addr,
            hiqlite_raft_addr,
            jwt_secret,
            repos_root_path,
            listen_addr,
            docker_socket_path,
            secrets_encryption_key,
            ssh_listen_addr,
            ssh_host_key_path,
            frontend_url,
            packages_root_path,
        })
    }
}

use anyhow::{Context, Result};

/// Runtime configuration for the server, loaded from environment variables.
#[derive(Debug, Clone)]
pub struct Config {
    pub database_url: String,
    pub jwt_secret: String,
    pub repos_root_path: String,
    pub listen_addr: String,
    pub docker_socket_path: String,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        let database_url = std::env::var("DATABASE_URL")
            .context("DATABASE_URL environment variable must be set")?;
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

        Ok(Config {
            database_url,
            jwt_secret,
            repos_root_path,
            listen_addr,
            docker_socket_path,
        })
    }
}

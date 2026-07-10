pub mod manager;
pub mod proxy;

pub use manager::{WorkspaceHandle, WorkspaceManager, WorkspaceStatus};

use thiserror::Error;

#[derive(Debug, Error)]
pub enum DevEnvError {
    #[error("docker error: {0}")]
    Docker(#[from] bollard::errors::Error),

    #[error("invalid workspace name: {0}")]
    InvalidName(String),

    #[error("workspace not found: {0}")]
    NotFound(String),

    #[error("proxy request error: {0}")]
    Proxy(#[from] reqwest::Error),

    #[error("invalid request: {0}")]
    InvalidRequest(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, DevEnvError>;

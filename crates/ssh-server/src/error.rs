/// Errors produced by the ssh-server crate.
///
/// Must implement `From<russh::Error>` so it can be used as
/// `russh::server::Handler::Error`.
#[derive(Debug, thiserror::Error)]
pub enum SshServerError {
    #[error("ssh protocol error: {0}")]
    Russh(#[from] russh::Error),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("database error: {0}")]
    Db(#[from] hiqlite::Error),

    #[error("git-core error: {0}")]
    GitCore(#[from] git_core::error::GitCoreError),

    #[error("other error: {0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, SshServerError>;

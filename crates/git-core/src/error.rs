use std::path::PathBuf;

/// Errors produced by the git-core crate.
#[derive(Debug, thiserror::Error)]
pub enum GitCoreError {
    #[error("invalid slug '{0}': must be a simple name (alphanumeric, '-', '_', '.') with no path traversal")]
    InvalidSlug(String),

    #[error("repository not found at {0}")]
    RepoNotFound(PathBuf),

    #[error("repository already exists at {0}")]
    RepoAlreadyExists(PathBuf),

    #[error("git operation failed: {0}")]
    Git(#[from] git2::Error),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("reference '{0}' not found")]
    RefNotFound(String),

    #[error("path '{0}' not found in tree")]
    PathNotFound(String),

    #[error("path '{0}' is a directory, not a file")]
    NotAFile(String),

    #[error("subprocess '{0}' failed: {1}")]
    Subprocess(String, String),

    #[error("no default branch could be determined")]
    NoDefaultBranch,

    #[error("unsupported git service: {0}")]
    UnsupportedService(String),

    #[error("utf8 conversion error: {0}")]
    Utf8(#[from] std::string::FromUtf8Error),

    #[error("merge conflict between '{0}' and '{1}': cannot merge automatically")]
    MergeConflict(String, String),
}

pub type Result<T> = std::result::Result<T, GitCoreError>;

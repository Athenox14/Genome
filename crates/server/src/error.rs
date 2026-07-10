use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

/// Top-level error type for HTTP handlers in the server binary.
#[derive(Debug, thiserror::Error)]
pub enum ServerError {
    #[error("not found: {0}")]
    NotFound(String),

    #[error("bad request: {0}")]
    BadRequest(String),

    #[error("unauthorized")]
    Unauthorized,

    #[error("conflict: {0}")]
    Conflict(String),

    #[error("git error: {0}")]
    Git(#[from] git_core::GitCoreError),

    #[error("dev-env error: {0}")]
    DevEnv(#[from] dev_env::DevEnvError),

    #[error("database error: {0}")]
    Db(#[from] hiqlite::Error),

    #[error("internal error: {0}")]
    Internal(#[from] anyhow::Error),
}

impl IntoResponse for ServerError {
    fn into_response(self) -> Response {
        let status = match &self {
            ServerError::NotFound(_) => StatusCode::NOT_FOUND,
            ServerError::BadRequest(_) => StatusCode::BAD_REQUEST,
            ServerError::Unauthorized => StatusCode::UNAUTHORIZED,
            ServerError::Conflict(_) => StatusCode::CONFLICT,
            ServerError::Git(git_core::GitCoreError::RepoNotFound(_))
            | ServerError::Git(git_core::GitCoreError::RefNotFound(_))
            | ServerError::Git(git_core::GitCoreError::PathNotFound(_)) => StatusCode::NOT_FOUND,
            ServerError::Git(git_core::GitCoreError::InvalidSlug(_)) => StatusCode::BAD_REQUEST,
            ServerError::Git(_) => StatusCode::INTERNAL_SERVER_ERROR,
            ServerError::DevEnv(dev_env::DevEnvError::NotFound(_)) => StatusCode::NOT_FOUND,
            ServerError::DevEnv(dev_env::DevEnvError::InvalidRequest(_))
            | ServerError::DevEnv(dev_env::DevEnvError::InvalidName(_)) => {
                StatusCode::BAD_REQUEST
            }
            ServerError::DevEnv(_) => StatusCode::INTERNAL_SERVER_ERROR,
            ServerError::Db(_) => StatusCode::INTERNAL_SERVER_ERROR,
            ServerError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        };

        (status, self.to_string()).into_response()
    }
}

pub type ServerResult<T> = std::result::Result<T, ServerError>;

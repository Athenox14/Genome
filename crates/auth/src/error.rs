use thiserror::Error;

#[derive(Debug, Error)]
pub enum AuthError {
    #[error("password hashing failed: {0}")]
    HashError(String),

    #[error("password verification failed: {0}")]
    VerifyError(String),

    #[error("invalid password hash")]
    InvalidHash,

    #[error("jwt creation failed: {0}")]
    JwtCreation(String),

    #[error("jwt verification failed: {0}")]
    JwtVerification(String),

    #[error("invalid ssh public key: {0}")]
    InvalidSshKey(String),

    #[error("invalid token")]
    InvalidToken,
}

pub type Result<T> = std::result::Result<T, AuthError>;

use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use rand::rngs::OsRng;

use crate::error::{AuthError, Result};

/// Hash a plaintext password using argon2id with a random salt.
pub fn hash_password(plain: &str) -> Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default();
    let hash = argon2
        .hash_password(plain.as_bytes(), &salt)
        .map_err(|e| AuthError::HashError(e.to_string()))?;
    Ok(hash.to_string())
}

/// Verify a plaintext password against a stored argon2 hash string.
pub fn verify_password(plain: &str, hash: &str) -> Result<bool> {
    let parsed_hash = PasswordHash::new(hash).map_err(|_| AuthError::InvalidHash)?;
    let argon2 = Argon2::default();
    match argon2.verify_password(plain.as_bytes(), &parsed_hash) {
        Ok(()) => Ok(true),
        Err(argon2::password_hash::Error::Password) => Ok(false),
        Err(e) => Err(AuthError::VerifyError(e.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_and_verify_roundtrip() {
        let hash = hash_password("correct horse battery staple").unwrap();
        assert!(verify_password("correct horse battery staple", &hash).unwrap());
        assert!(!verify_password("wrong password", &hash).unwrap());
    }
}

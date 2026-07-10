use chrono::Utc;
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::{AuthError, Result};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    pub sub: Uuid,
    pub username: String,
    pub is_admin: bool,
    pub exp: usize,
    pub iat: usize,
}

/// Input used to construct a fresh set of claims before signing.
#[derive(Debug, Clone)]
pub struct ClaimsInput {
    pub sub: Uuid,
    pub username: String,
    pub is_admin: bool,
}

impl From<&entity::user::Model> for ClaimsInput {
    fn from(user: &entity::user::Model) -> Self {
        ClaimsInput {
            sub: user.id,
            username: user.username.clone(),
            is_admin: user.is_admin,
        }
    }
}

pub fn create_jwt(claims_input: ClaimsInput, secret: &str, ttl_hours: i64) -> Result<String> {
    let now = Utc::now();
    let iat = now.timestamp();
    let exp = now
        .checked_add_signed(chrono::Duration::hours(ttl_hours))
        .ok_or_else(|| AuthError::JwtCreation("ttl overflow".to_string()))?
        .timestamp();

    let claims = Claims {
        sub: claims_input.sub,
        username: claims_input.username,
        is_admin: claims_input.is_admin,
        iat: iat as usize,
        exp: exp as usize,
    };

    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .map_err(|e| AuthError::JwtCreation(e.to_string()))
}

pub fn verify_jwt(token: &str, secret: &str) -> Result<Claims> {
    let data = decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &Validation::default(),
    )
    .map_err(|e| AuthError::JwtVerification(e.to_string()))?;
    Ok(data.claims)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_and_verify_roundtrip() {
        let input = ClaimsInput {
            sub: Uuid::new_v4(),
            username: "alice".to_string(),
            is_admin: false,
        };
        let token = create_jwt(input.clone(), "secret", 1).unwrap();
        let claims = verify_jwt(&token, "secret").unwrap();
        assert_eq!(claims.sub, input.sub);
        assert_eq!(claims.username, "alice");
        assert!(!claims.is_admin);
    }

    #[test]
    fn wrong_secret_fails() {
        let input = ClaimsInput {
            sub: Uuid::new_v4(),
            username: "alice".to_string(),
            is_admin: false,
        };
        let token = create_jwt(input, "secret", 1).unwrap();
        assert!(verify_jwt(&token, "other-secret").is_err());
    }
}

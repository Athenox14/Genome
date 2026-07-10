use async_trait::async_trait;
use http::HeaderMap;

use crate::jwt::{verify_jwt, Claims};
use crate::token::hash_token;

/// Implemented by the caller (which owns DB access) to resolve a hashed
/// personal access token into the `Claims` of the user it belongs to.
#[async_trait]
pub trait TokenLookup: Send + Sync {
    async fn lookup(&self, token_hash: &str) -> Option<Claims>;
}

/// Extract an authenticated user's claims from request headers.
///
/// Supports two schemes in the `Authorization` header:
/// - `Bearer <jwt>` — verified against `jwt_secret`.
/// - `token <pat>` — hashed and resolved via `token_lookup`, if provided.
pub async fn extract_user_from_headers(
    headers: &HeaderMap,
    jwt_secret: &str,
    token_lookup: Option<&dyn TokenLookup>,
) -> Option<Claims> {
    let auth_header = headers.get(http::header::AUTHORIZATION)?.to_str().ok()?;

    if let Some(jwt) = auth_header.strip_prefix("Bearer ") {
        return verify_jwt(jwt.trim(), jwt_secret).ok();
    }

    if let Some(pat) = auth_header.strip_prefix("token ") {
        let lookup = token_lookup?;
        let hash = hash_token(pat.trim());
        return lookup.lookup(&hash).await;
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jwt::{create_jwt, ClaimsInput};
    use http::header::AUTHORIZATION;
    use uuid::Uuid;

    struct NoopLookup;

    #[async_trait]
    impl TokenLookup for NoopLookup {
        async fn lookup(&self, _token_hash: &str) -> Option<Claims> {
            None
        }
    }

    #[tokio::test]
    async fn extracts_from_bearer_jwt() {
        let input = ClaimsInput {
            sub: Uuid::new_v4(),
            username: "alice".to_string(),
            is_admin: false,
        };
        let token = create_jwt(input.clone(), "secret", 1).unwrap();

        let mut headers = HeaderMap::new();
        headers.insert(AUTHORIZATION, format!("Bearer {token}").parse().unwrap());

        let claims = extract_user_from_headers(&headers, "secret", None)
            .await
            .expect("should extract claims");
        assert_eq!(claims.sub, input.sub);
    }

    #[tokio::test]
    async fn returns_none_without_header() {
        let headers = HeaderMap::new();
        assert!(extract_user_from_headers(&headers, "secret", None)
            .await
            .is_none());
    }

    #[tokio::test]
    async fn pat_without_lookup_returns_none() {
        let mut headers = HeaderMap::new();
        headers.insert(AUTHORIZATION, "token sometoken".parse().unwrap());
        assert!(extract_user_from_headers(&headers, "secret", None)
            .await
            .is_none());
    }

    #[tokio::test]
    async fn pat_with_lookup_that_finds_nothing_returns_none() {
        let mut headers = HeaderMap::new();
        headers.insert(AUTHORIZATION, "token sometoken".parse().unwrap());
        let lookup = NoopLookup;
        assert!(
            extract_user_from_headers(&headers, "secret", Some(&lookup))
                .await
                .is_none()
        );
    }
}

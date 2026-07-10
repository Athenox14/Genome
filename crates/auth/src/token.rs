use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use rand::RngCore;
use sha2::{Digest, Sha256};

const TOKEN_PREFIX: &str = "genome_pat_";
const TOKEN_BYTES: usize = 32;

/// Generate a new personal access token.
///
/// Returns `(plaintext_token, sha256_hash_hex)`. The plaintext is shown to the
/// user exactly once; only the hash should be persisted to the database.
pub fn generate_access_token() -> (String, String) {
    let mut bytes = [0u8; TOKEN_BYTES];
    rand::thread_rng().fill_bytes(&mut bytes);
    let random_part = URL_SAFE_NO_PAD.encode(bytes);
    let plaintext = format!("{TOKEN_PREFIX}{random_part}");
    let hash = hash_token(&plaintext);
    (plaintext, hash)
}

/// Hex-encoded SHA256 hash of a token, for lookup/comparison against stored hashes.
pub fn hash_token(token: &str) -> String {
    let digest = Sha256::digest(token.as_bytes());
    hex_encode(&digest)
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generates_distinct_tokens() {
        let (t1, h1) = generate_access_token();
        let (t2, h2) = generate_access_token();
        assert_ne!(t1, t2);
        assert_ne!(h1, h2);
        assert!(t1.starts_with(TOKEN_PREFIX));
    }

    #[test]
    fn hash_is_deterministic() {
        let (plaintext, hash) = generate_access_token();
        assert_eq!(hash_token(&plaintext), hash);
    }
}

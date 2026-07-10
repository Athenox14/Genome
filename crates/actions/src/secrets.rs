//! Encryption at rest for repository Actions secrets.
//!
//! Values are encrypted with AES-256-GCM using a key derived from the
//! `SECRETS_ENCRYPTION_KEY` environment variable (32 raw bytes, base64
//! encoded). The random nonce used for each encryption is prepended to the
//! returned ciphertext so it can be recovered on decrypt.

use aes_gcm::aead::{Aead, AeadCore, KeyInit, OsRng};
use aes_gcm::{Aes256Gcm, Key};
use anyhow::{anyhow, Result};

const NONCE_LEN: usize = 12;

/// Encrypt `plaintext` with the given 32-byte key. Returns `nonce || ciphertext`.
pub fn encrypt_secret(key: &[u8; 32], plaintext: &str) -> Vec<u8> {
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key));
    let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
    let ciphertext = cipher
        .encrypt(&nonce, plaintext.as_bytes())
        .expect("AES-GCM encryption failure is not expected for in-memory buffers");

    let mut out = Vec::with_capacity(NONCE_LEN + ciphertext.len());
    out.extend_from_slice(nonce.as_slice());
    out.extend_from_slice(&ciphertext);
    out
}

/// Decrypt a buffer produced by [`encrypt_secret`].
pub fn decrypt_secret(key: &[u8; 32], ciphertext: &[u8]) -> Result<String> {
    if ciphertext.len() < NONCE_LEN {
        return Err(anyhow!("ciphertext too short to contain a nonce"));
    }
    let (nonce_bytes, ct) = ciphertext.split_at(NONCE_LEN);
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key));
    let plaintext = cipher
        .decrypt(nonce_bytes.into(), ct)
        .map_err(|e| anyhow!("failed to decrypt secret: {e}"))?;
    String::from_utf8(plaintext).map_err(|e| anyhow!("decrypted secret is not valid UTF-8: {e}"))
}

/// Parse the `SECRETS_ENCRYPTION_KEY` environment variable (32 bytes,
/// base64-encoded) into a raw key.
pub fn key_from_base64(encoded: &str) -> Result<[u8; 32]> {
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded.trim())
        .map_err(|e| anyhow!("SECRETS_ENCRYPTION_KEY is not valid base64: {e}"))?;
    let arr: [u8; 32] = bytes
        .try_into()
        .map_err(|_| anyhow!("SECRETS_ENCRYPTION_KEY must decode to exactly 32 bytes"))?;
    Ok(arr)
}

/// Substitute `${{ secrets.NAME }}` occurrences (simple string replace, not a
/// full expression engine) in a workflow step's `run:` command with the
/// decrypted secret values.
pub fn substitute_secrets(command: &str, secrets: &std::collections::HashMap<String, String>) -> String {
    let mut out = command.to_string();
    for (name, value) in secrets {
        for pattern in [
            format!("${{{{ secrets.{name} }}}}"),
            format!("${{{{secrets.{name}}}}}"),
            format!("${{{{ secrets.{name}}}}}"),
            format!("${{{{secrets.{name} }}}}"),
        ] {
            out = out.replace(&pattern, value);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let key = [7u8; 32];
        let ct = encrypt_secret(&key, "hello world");
        let pt = decrypt_secret(&key, &ct).unwrap();
        assert_eq!(pt, "hello world");
    }

    #[test]
    fn substitution() {
        let mut secrets = std::collections::HashMap::new();
        secrets.insert("TOKEN".to_string(), "abc123".to_string());
        let out = substitute_secrets("curl -H \"Authorization: ${{ secrets.TOKEN }}\"", &secrets);
        assert_eq!(out, "curl -H \"Authorization: abc123\"");
    }
}

//! TOTP-based two-factor authentication, using the `totp-rs` crate.
//!
//! Secrets are stored base32-encoded on the user row (`users.totp_secret`);
//! `totp_enabled` gates whether `login` requires a code.

use totp_rs::{Algorithm, Secret, TOTP};

const ISSUER: &str = "Genome";

/// Generates a new random base32-encoded TOTP secret suitable for storage
/// and for building a provisioning URI.
pub fn generate_totp_secret() -> String {
    Secret::generate_secret().to_encoded().to_string()
}

fn build_totp(secret: &str, username: &str) -> Option<TOTP> {
    let secret_bytes = Secret::Encoded(secret.to_string()).to_bytes().ok()?;
    TOTP::new(
        Algorithm::SHA1,
        6,
        1,
        30,
        secret_bytes,
        Some(ISSUER.to_string()),
        username.to_string(),
    )
    .ok()
}

/// Builds the `otpauth://` provisioning URI for the given secret, suitable
/// for rendering as a QR code on the frontend.
pub fn totp_provisioning_uri(secret: &str, username: &str, issuer: &str) -> String {
    let secret_bytes = match Secret::Encoded(secret.to_string()).to_bytes() {
        Ok(b) => b,
        Err(_) => return String::new(),
    };
    match TOTP::new(
        Algorithm::SHA1,
        6,
        1,
        30,
        secret_bytes,
        Some(issuer.to_string()),
        username.to_string(),
    ) {
        Ok(totp) => totp.get_url(),
        Err(_) => String::new(),
    }
}

/// Verifies a user-supplied TOTP code against the given secret, allowing for
/// +/- one time step of clock skew.
pub fn verify_totp(secret: &str, code: &str) -> bool {
    match build_totp(secret, "user") {
        Some(totp) => totp.check_current(code).unwrap_or(false),
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_secret_round_trips() {
        let secret = generate_totp_secret();
        let uri = totp_provisioning_uri(&secret, "alice", ISSUER);
        assert!(uri.starts_with("otpauth://totp/"));

        let totp = build_totp(&secret, "alice").expect("valid totp");
        let code = totp.generate_current().expect("generate code");
        assert!(verify_totp(&secret, &code));
        assert!(!verify_totp(&secret, "000000") || code == "000000");
    }
}

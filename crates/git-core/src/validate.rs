use crate::error::{GitCoreError, Result};

/// Validate that a string is a safe "slug" usable as a single path component:
/// non-empty, no path separators, no leading dot, no `..`, and restricted to
/// a conservative character set. This prevents path traversal when building
/// filesystem paths from user-supplied owner/repo names.
pub fn validate_slug(value: &str) -> Result<()> {
    if value.is_empty() {
        return Err(GitCoreError::InvalidSlug(value.to_string()));
    }
    if value == "." || value == ".." {
        return Err(GitCoreError::InvalidSlug(value.to_string()));
    }
    if value.starts_with('.') {
        return Err(GitCoreError::InvalidSlug(value.to_string()));
    }
    if value.contains("..") {
        return Err(GitCoreError::InvalidSlug(value.to_string()));
    }
    if value.contains('/') || value.contains('\\') {
        return Err(GitCoreError::InvalidSlug(value.to_string()));
    }
    let valid = value
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.');
    if !valid {
        return Err(GitCoreError::InvalidSlug(value.to_string()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_simple_names() {
        assert!(validate_slug("my-repo_1.0").is_ok());
    }

    #[test]
    fn rejects_traversal() {
        assert!(validate_slug("..").is_err());
        assert!(validate_slug("../etc").is_err());
        assert!(validate_slug("a/../b").is_err());
        assert!(validate_slug("/etc/passwd").is_err());
        assert!(validate_slug(".hidden").is_err());
        assert!(validate_slug("").is_err());
    }
}

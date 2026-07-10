#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize)]
pub enum Permission {
    Read,
    Write,
    Admin,
}

/// Determine the effective permission a principal has on a repository.
///
/// Rules:
/// - Owners and org-admins always get `Admin`.
/// - An explicit collaborator permission (if any) applies as-is.
/// - Private repos with no applicable grant => `None` (no access).
/// - Public repos default to `Read` for anyone, absent a higher explicit grant.
pub fn effective_permission(
    is_owner: bool,
    is_admin_org_role: bool,
    collaborator_perm: Option<Permission>,
    repo_is_private: bool,
) -> Option<Permission> {
    if is_owner || is_admin_org_role {
        return Some(Permission::Admin);
    }

    if let Some(perm) = collaborator_perm {
        return Some(perm);
    }

    if repo_is_private {
        None
    } else {
        Some(Permission::Read)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordering_holds() {
        assert!(Permission::Read < Permission::Write);
        assert!(Permission::Write < Permission::Admin);
    }

    #[test]
    fn owner_always_admin() {
        assert_eq!(
            effective_permission(true, false, None, true),
            Some(Permission::Admin)
        );
    }

    #[test]
    fn org_admin_always_admin() {
        assert_eq!(
            effective_permission(false, true, Some(Permission::Read), true),
            Some(Permission::Admin)
        );
    }

    #[test]
    fn explicit_collaborator_perm_applies() {
        assert_eq!(
            effective_permission(false, false, Some(Permission::Write), true),
            Some(Permission::Write)
        );
    }

    #[test]
    fn private_repo_no_perm_denies_access() {
        assert_eq!(effective_permission(false, false, None, true), None);
    }

    #[test]
    fn public_repo_defaults_to_read() {
        assert_eq!(
            effective_permission(false, false, None, false),
            Some(Permission::Read)
        );
    }
}

//! `git-core`: repository storage, read APIs, and smart-HTTP git protocol
//! support for the Genome git forge.

pub mod error;
pub mod hooks;
pub mod manager;
pub mod smart_http;
pub mod types;
pub mod validate;

pub use error::{GitCoreError, Result};
pub use hooks::{compute_ref_changes, diff_refs, CommitHookSink};
pub use manager::RepoManager;
pub use smart_http::{
    handle_info_refs, handle_service_rpc, info_refs_content_type, service_rpc_content_type,
    GitService,
};
pub use types::{CommitInfo, EntryKind, TreeEntry};
pub use validate::validate_slug;

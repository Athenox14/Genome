use chrono::{DateTime, Utc};

/// The kind of object a tree entry refers to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    Blob,
    Tree,
}

impl EntryKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            EntryKind::Blob => "blob",
            EntryKind::Tree => "tree",
        }
    }
}

/// A single entry in a git tree listing.
#[derive(Debug, Clone)]
pub struct TreeEntry {
    pub name: String,
    pub path: String,
    pub kind: EntryKind,
    pub size: Option<u64>,
    pub oid: String,
}

/// Summary information about a single commit.
#[derive(Debug, Clone)]
pub struct CommitInfo {
    pub sha: String,
    pub author_name: String,
    pub author_email: String,
    pub message: String,
    pub timestamp: DateTime<Utc>,
}

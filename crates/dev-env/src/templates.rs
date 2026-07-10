use crate::manager::IMAGE_CODE_SERVER;

/// A named, built-in workspace configuration: an image plus the set of named
/// ports it exposes (empty if the image is headless / exec-only, e.g. a
/// plain language runtime with no bundled web UI).
pub struct WorkspaceTemplate {
    pub name: &'static str,
    pub image: &'static str,
    pub ports: &'static [(&'static str, u16)],
}

pub const TEMPLATES: &[WorkspaceTemplate] = &[
    WorkspaceTemplate {
        name: "code-server",
        image: IMAGE_CODE_SERVER,
        ports: &[("http", 8080)],
    },
    // Headless Rust toolchain image; no bundled web UI, so it exposes no
    // ports and is only reachable via `exec_command`/SSH.
    WorkspaceTemplate {
        name: "rust-dev",
        image: "rust:1-bookworm",
        ports: &[],
    },
    // Headless Node.js toolchain image; same exec-only shape as "rust-dev".
    WorkspaceTemplate {
        name: "node-dev",
        image: "node:20-bookworm",
        ports: &[],
    },
];

pub fn find_template(name: &str) -> Option<&'static WorkspaceTemplate> {
    TEMPLATES.iter().find(|t| t.name == name)
}

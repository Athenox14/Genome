use std::sync::Arc;

/// Global application context shared across all GraphQL requests.
///
/// `hiqlite::Client` is cheaply `Clone` (it wraps its own internal shared
/// state), so it is stored directly rather than behind an extra `Arc`.
#[derive(Clone)]
pub struct AppContext {
    pub db: hiqlite::Client,
    pub repo_manager: Arc<git_core::RepoManager>,
    pub jwt_secret: String,
    pub actions_executor: Arc<actions::Executor>,
    pub workspace_manager: Arc<dev_env::WorkspaceManager>,
    pub webhook_dispatcher: Arc<webhooks::WebhookDispatcher>,
}

/// Per-request context, holding the authenticated user's claims (if any).
#[derive(Clone, Default)]
pub struct RequestContext {
    pub user: Option<auth::Claims>,
}

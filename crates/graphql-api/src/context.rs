use std::sync::Arc;

use sea_orm::DatabaseConnection;

/// Global application context shared across all GraphQL requests.
#[derive(Clone)]
pub struct AppContext {
    pub db: DatabaseConnection,
    pub repo_manager: Arc<git_core::RepoManager>,
    pub jwt_secret: String,
    pub actions_executor: Arc<actions::Executor>,
    pub workspace_manager: Arc<dev_env::WorkspaceManager>,
}

/// Per-request context, holding the authenticated user's claims (if any).
#[derive(Clone, Default)]
pub struct RequestContext {
    pub user: Option<auth::Claims>,
}

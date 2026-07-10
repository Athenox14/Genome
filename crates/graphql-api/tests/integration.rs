//! Integration tests for `graphql-api` that exercise real GraphQL
//! mutations/queries against a live Postgres database (schema brought up to
//! date via `migration::Migrator::up`) and (for repository creation) a real
//! on-disk bare git repo via `git-core`.
//!
//! These require `DATABASE_URL` to point at a reachable Postgres instance
//! (the CI workflow's `postgres:16` service container provides one; see
//! `.github/workflows/docker-publish.yml`). Locally, if no database is
//! reachable, these tests will fail at connection time -- that's expected
//! and is why CI (not this dev machine, whose Docker networking is broken)
//! is the source of truth for whether they pass.
//!
//! Test isolation: rather than wrapping each test in a DB transaction, every
//! test generates unique usernames/repo names via `uuid::Uuid::new_v4()` so
//! that tests can run concurrently (the default `cargo test` behavior)
//! without colliding on unique constraints.

use std::sync::Arc;

use async_graphql::Request;
use migration::MigratorTrait;
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter};
use serde_json::json;
use uuid::Uuid;

use graphql_api::{build_schema, AppContext, GraphQLSchema, RequestContext};

/// Builds a fresh `AppContext` (and schema) wired up to the test database,
/// with `RepoManager` pointed at a throwaway temp directory so created bare
/// repos don't pollute `./data/repos`, and the actions/dev-env managers
/// constructed in a way that doesn't require a reachable Docker daemon at
/// context-construction time.
async fn test_schema() -> (GraphQLSchema, AppContext, tempfile::TempDir) {
    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://genome:genome@localhost:5432/genome".to_string());

    let db: DatabaseConnection = sea_orm::Database::connect(&database_url)
        .await
        .expect("failed to connect to test database (is DATABASE_URL reachable?)");

    migration::Migrator::up(&db, None)
        .await
        .expect("failed to run migrations against test database");

    let repos_tmp = tempfile::tempdir().expect("failed to create temp dir for repos root");
    let artifacts_tmp = tempfile::tempdir().expect("failed to create temp dir for artifacts root");

    let repo_manager = Arc::new(git_core::RepoManager::new(repos_tmp.path().to_path_buf()));
    let actions_executor = Arc::new(
        actions::Executor::new(artifacts_tmp.path().to_path_buf())
            .expect("failed to construct actions::Executor"),
    );
    // `connect_local` only builds a Docker client handle; it does not
    // perform any I/O against a daemon, so this succeeds even when Docker
    // Desktop is unreachable (which it currently is on this dev machine).
    let workspace_manager = Arc::new(
        dev_env::WorkspaceManager::connect_local().expect("failed to construct WorkspaceManager"),
    );
    let webhook_dispatcher = Arc::new(webhooks::WebhookDispatcher::new(db.clone()));

    let app_ctx = AppContext {
        db,
        repo_manager,
        jwt_secret: "test-secret-for-integration-tests".to_string(),
        actions_executor,
        workspace_manager,
        webhook_dispatcher,
    };

    let schema = build_schema(app_ctx.clone());
    (schema, app_ctx, repos_tmp)
}

/// Executes a GraphQL request with no authenticated user attached.
async fn exec(schema: &GraphQLSchema, query: &str) -> async_graphql::Response {
    let req = Request::new(query).data(RequestContext { user: None });
    schema.execute(req).await
}

/// Executes a GraphQL request as the given authenticated user. This mirrors
/// what the server's `graphql_post_handler`/`graphql_handler` do after
/// resolving an `Authorization` header (via JWT or, for PATs, via
/// `DbTokenLookup`) into `Claims` -- here we construct/obtain the `Claims`
/// directly and inject them as a per-request `RequestContext`, bypassing the
/// HTTP header-parsing layer entirely since `Schema::execute` needs no HTTP.
async fn exec_as(
    schema: &GraphQLSchema,
    query: &str,
    claims: auth::Claims,
) -> async_graphql::Response {
    let req = Request::new(query).data(RequestContext { user: Some(claims) });
    schema.execute(req).await
}

fn unique(prefix: &str) -> String {
    format!("{prefix}_{}", Uuid::new_v4().simple())
}

fn assert_no_errors(resp: &async_graphql::Response) {
    assert!(
        resp.errors.is_empty(),
        "expected no GraphQL errors, got: {:?}",
        resp.errors
    );
}

fn data_json(resp: async_graphql::Response) -> serde_json::Value {
    resp.data
        .into_json()
        .expect("failed to serialize response data to JSON")
}

/// Registers a user via the `register` mutation, then fetches the inserted
/// row back from the DB directly so the caller has the full
/// `entity::user::Model` (in particular `id`) for constructing `Claims`.
async fn register_user(
    schema: &GraphQLSchema,
    app: &AppContext,
    username: &str,
    email: &str,
) -> entity::user::Model {
    let register_query = format!(
        r#"mutation {{
            register(username: "{username}", email: "{email}", password: "hunter2222") {{
                id
                username
            }}
        }}"#
    );
    let resp = exec(schema, &register_query).await;
    assert_no_errors(&resp);
    let data = data_json(resp);
    assert_eq!(data["register"]["username"], json!(username));

    entity::prelude::User::find()
        .filter(entity::user::Column::Username.eq(username))
        .one(&app.db)
        .await
        .expect("db query failed")
        .expect("user row should exist right after register")
}

fn claims_for(user: &entity::user::Model) -> auth::Claims {
    auth::Claims {
        sub: user.id,
        username: user.username.clone(),
        is_admin: user.is_admin,
        exp: usize::MAX,
        iat: 0,
    }
}

/// Registers a user, then verifies `login` returns a JWT whose claims decode
/// to the expected subject/username -- the core register -> login flow.
#[tokio::test]
async fn register_then_login_returns_valid_jwt() {
    let (schema, app, _tmp) = test_schema().await;

    let username = unique("regtest");
    let email = format!("{username}@example.com");
    let user = register_user(&schema, &app, &username, &email).await;

    let login_query = format!(
        r#"mutation {{
            login(username: "{username}", password: "hunter2222") {{
                token
                user {{ username }}
            }}
        }}"#
    );
    let resp = exec(&schema, &login_query).await;
    assert_no_errors(&resp);
    let data = data_json(resp);
    let token = data["login"]["token"]
        .as_str()
        .expect("token should be a string")
        .to_string();
    assert_eq!(data["login"]["user"]["username"], json!(username));

    let claims = auth::verify_jwt(&token, "test-secret-for-integration-tests")
        .expect("JWT returned by login should verify against the app's jwt_secret");
    assert_eq!(claims.username, username);
    assert_eq!(claims.sub, user.id);
}

/// `login` with a wrong password must be rejected.
#[tokio::test]
async fn login_with_wrong_password_is_rejected() {
    let (schema, app, _tmp) = test_schema().await;

    let username = unique("badlogin");
    let email = format!("{username}@example.com");
    register_user(&schema, &app, &username, &email).await;

    let login_query = format!(
        r#"mutation {{
            login(username: "{username}", password: "totally-wrong-password") {{
                token
            }}
        }}"#
    );
    let resp = exec(&schema, &login_query).await;
    assert!(
        !resp.errors.is_empty(),
        "login with a wrong password should return a GraphQL error"
    );
}

/// Regression test for the personal-access-token auth path: create a PAT via
/// the `createAccessToken` mutation, then resolve it back into `Claims` the
/// same way the server's `DbTokenLookup` does (hash -> lookup `access_token`
/// row -> load owning user), and confirm those PAT-derived claims can
/// authenticate a subsequent mutation.
#[tokio::test]
async fn create_access_token_then_pat_resolved_claims_authenticate() {
    let (schema, app, _tmp) = test_schema().await;

    let username = unique("pattest");
    let email = format!("{username}@example.com");
    let user = register_user(&schema, &app, &username, &email).await;
    let claims = claims_for(&user);

    let create_pat_query = r#"mutation {
        createAccessToken(name: "ci-token") {
            token
            accessToken { id name }
        }
    }"#;
    let resp = exec_as(&schema, create_pat_query, claims.clone()).await;
    assert_no_errors(&resp);
    let data = data_json(resp);
    let pat_plaintext = data["createAccessToken"]["token"]
        .as_str()
        .expect("createAccessToken should return the plaintext token")
        .to_string();
    assert!(!pat_plaintext.is_empty());

    // Resolve the PAT back into a DB row the same way `DbTokenLookup::lookup`
    // does in `crates/server/src/main.rs`: hash the plaintext and look up the
    // `access_token` row by `token_hash`.
    let token_hash = auth::hash_token(&pat_plaintext);
    let token_row = entity::prelude::AccessToken::find()
        .filter(entity::access_token::Column::TokenHash.eq(token_hash))
        .one(&app.db)
        .await
        .expect("db query failed")
        .expect("access token row should exist after createAccessToken");
    assert_eq!(token_row.user_id, user.id);

    let resolved_user = entity::prelude::User::find_by_id(token_row.user_id)
        .one(&app.db)
        .await
        .expect("db query failed")
        .expect("owning user should exist");
    let resolved_claims = claims_for(&resolved_user);

    // Use the PAT-resolved claims to perform an authenticated mutation,
    // proving the PAT -> Claims resolution path actually authorizes
    // requests (this is the flow that was previously broken).
    let repo_name = unique("patrepo");
    let create_repo_query = format!(
        r#"mutation {{
            createRepository(name: "{repo_name}", isPrivate: false) {{
                id
                name
            }}
        }}"#
    );
    let resp = exec_as(&schema, &create_repo_query, resolved_claims).await;
    assert_no_errors(&resp);
    let data = data_json(resp);
    assert_eq!(data["createRepository"]["name"], json!(repo_name));
}

/// `createRepository` must both insert a `repository` row AND create a real
/// bare git repository on disk via `git-core` (in the temp `REPOS_ROOT_PATH`
/// used for this test, not the real `./data/repos`).
#[tokio::test]
async fn create_repository_inserts_row_and_creates_bare_repo_on_disk() {
    let (schema, app, repos_tmp) = test_schema().await;

    let username = unique("repoowner");
    let email = format!("{username}@example.com");
    let user = register_user(&schema, &app, &username, &email).await;
    let claims = claims_for(&user);

    let repo_name = unique("myrepo");
    let create_repo_query = format!(
        r#"mutation {{
            createRepository(name: "{repo_name}", description: "test repo", isPrivate: true) {{
                id
                name
                isPrivate
            }}
        }}"#
    );
    let resp = exec_as(&schema, &create_repo_query, claims).await;
    assert_no_errors(&resp);
    let data = data_json(resp);
    assert_eq!(data["createRepository"]["name"], json!(repo_name));
    assert_eq!(data["createRepository"]["isPrivate"], json!(true));

    // DB row exists.
    let repo_row = entity::prelude::Repository::find()
        .filter(entity::repository::Column::Name.eq(repo_name.clone()))
        .filter(entity::repository::Column::OwnerId.eq(user.id))
        .one(&app.db)
        .await
        .expect("db query failed")
        .expect("repository row should exist after createRepository");
    assert!(repo_row.is_private);

    // Real bare repo on disk: `RepoManager` lays repos out as
    // `{root}/{owner}/{name}.git`.
    let expected_repo_path = repos_tmp.path().join(&username).join(format!("{repo_name}.git"));
    assert!(
        expected_repo_path.is_dir(),
        "expected bare repo directory to exist at {expected_repo_path:?}"
    );
    assert!(
        expected_repo_path.join("HEAD").is_file(),
        "expected bare repo to contain a HEAD file (i.e. actually be a git repo), checked at {expected_repo_path:?}"
    );
}

/// `createIssue` -> `updateIssue` (closing it) lifecycle: confirms the issue
/// starts open, and that setting `state: "closed"` actually persists the
/// state change (and sets `closedAt`).
#[tokio::test]
async fn create_issue_then_close_it_updates_state() {
    let (schema, app, _tmp) = test_schema().await;

    let username = unique("issueuser");
    let email = format!("{username}@example.com");
    let user = register_user(&schema, &app, &username, &email).await;
    let claims = claims_for(&user);

    let repo_name = unique("issuerepo");
    let create_repo_query = format!(
        r#"mutation {{ createRepository(name: "{repo_name}", isPrivate: false) {{ id }} }}"#
    );
    let resp = exec_as(&schema, &create_repo_query, claims.clone()).await;
    assert_no_errors(&resp);
    let repo_id = data_json(resp)["createRepository"]["id"]
        .as_str()
        .expect("repo id should be a string")
        .to_string();

    let create_issue_query = format!(
        r#"mutation {{
            createIssue(repoId: "{repo_id}", title: "Something is broken", body: "details here") {{
                id
                number
                state
            }}
        }}"#
    );
    let resp = exec_as(&schema, &create_issue_query, claims.clone()).await;
    assert_no_errors(&resp);
    let data = data_json(resp);
    assert_eq!(data["createIssue"]["state"], json!("open"));
    let issue_id = data["createIssue"]["id"]
        .as_str()
        .expect("issue id should be a string")
        .to_string();

    let update_issue_query = format!(
        r#"mutation {{
            updateIssue(issueId: "{issue_id}", state: "closed") {{
                id
                state
            }}
        }}"#
    );
    let resp = exec_as(&schema, &update_issue_query, claims).await;
    assert_no_errors(&resp);
    let data = data_json(resp);
    assert_eq!(data["updateIssue"]["state"], json!("closed"));

    let issue_row = entity::prelude::Issue::find_by_id(Uuid::parse_str(&issue_id).unwrap())
        .one(&app.db)
        .await
        .expect("db query failed")
        .expect("issue row should exist");
    assert_eq!(issue_row.state, entity::issue::state::CLOSED);
    assert!(issue_row.closed_at.is_some());
}

/// A user with no relationship to a private repository must not be able to
/// create an issue on it -- exercises the `repo_permission` check on the
/// `createIssue` mutation as a stand-in "gated mutation rejection" case
/// (simpler to set up deterministically than a full branch-protection +
/// PR-review-count `mergePullRequest` scenario, and covers the same
/// authorization-gate code path).
#[tokio::test]
async fn create_issue_on_private_repo_by_unrelated_user_is_forbidden() {
    let (schema, app, _tmp) = test_schema().await;

    let owner_username = unique("privowner");
    let owner_email = format!("{owner_username}@example.com");
    let owner = register_user(&schema, &app, &owner_username, &owner_email).await;
    let owner_claims = claims_for(&owner);

    let repo_name = unique("privaterepo");
    let create_repo_query = format!(
        r#"mutation {{ createRepository(name: "{repo_name}", isPrivate: true) {{ id }} }}"#
    );
    let resp = exec_as(&schema, &create_repo_query, owner_claims).await;
    assert_no_errors(&resp);
    let repo_id = data_json(resp)["createRepository"]["id"]
        .as_str()
        .expect("repo id should be a string")
        .to_string();

    let outsider_username = unique("outsider");
    let outsider_email = format!("{outsider_username}@example.com");
    let outsider = register_user(&schema, &app, &outsider_username, &outsider_email).await;
    let outsider_claims = claims_for(&outsider);

    let create_issue_query = format!(
        r#"mutation {{
            createIssue(repoId: "{repo_id}", title: "should not be allowed") {{
                id
            }}
        }}"#
    );
    let resp = exec_as(&schema, &create_issue_query, outsider_claims).await;
    assert!(
        !resp.errors.is_empty(),
        "an unrelated user should be forbidden from creating an issue on a private repo"
    );
}

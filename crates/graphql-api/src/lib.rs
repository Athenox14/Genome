pub mod context;
pub mod mutation;
pub mod query;
pub mod types;

use async_graphql::{EmptySubscription, Schema};
use async_graphql_axum::{GraphQLRequest, GraphQLResponse};
use axum::extract::State;
use axum::http::HeaderMap;

pub use context::{AppContext, RequestContext};
pub use mutation::MutationRoot;
pub use query::QueryRoot;

pub type GraphQLSchema = Schema<QueryRoot, MutationRoot, EmptySubscription>;

/// Build the GraphQL schema, embedding the shared [`AppContext`] as global
/// schema data (available in every resolver via `ctx.data::<AppContext>()`).
pub fn build_schema(ctx: AppContext) -> GraphQLSchema {
    Schema::build(QueryRoot, MutationRoot, EmptySubscription)
        .data(ctx)
        .finish()
}

/// Axum handler that authenticates the request (via `Authorization` header,
/// supporting both `Bearer <jwt>` and `token <pat>` schemes) and executes the
/// GraphQL request against the schema, with a per-request [`RequestContext`]
/// carrying the resolved user claims (if any).
pub async fn graphql_handler(
    State(schema): State<GraphQLSchema>,
    headers: HeaderMap,
    req: GraphQLRequest,
) -> GraphQLResponse {
    let jwt_secret = {
        // The schema carries a clone of AppContext as global data; retrieve it
        // to look up the configured JWT secret for verification.
        schema
            .data::<AppContext>()
            .map(|app| app.jwt_secret.clone())
            .unwrap_or_default()
    };

    let user = auth::extract_user_from_headers(&headers, &jwt_secret, None).await;

    let request_ctx = RequestContext { user };
    let mut request = req.into_inner();
    request = request.data(request_ctx);

    schema.execute(request).await.into()
}

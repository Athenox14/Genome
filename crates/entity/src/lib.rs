//! # Entity layer (Hiqlite / SQLite backend)
//!
//! This crate defines the persistence-layer data structures for Genome. It
//! previously used SeaORM's `Entity` / `Model` / `ActiveModel` / `Column`
//! split against Postgres; the whole stack has since moved to
//! [`hiqlite`](https://github.com/sebadob/hiqlite), an embeddable,
//! Raft-replicated SQLite database. Hiqlite has no ORM layer of its own — a
//! `hiqlite::Client` executes raw SQL and maps rows onto plain Rust structs
//! via `serde::Deserialize` (`Client::query_as::<T>()` /
//! `Client::query_as_one::<T>()`) or via `From<&mut hiqlite::Row<'_>>`
//! (`Client::query_map`), plus `Client::execute()` / `Client::execute_returning_map_one()`
//! / `Client::txn()` for writes. See `hiqlite`'s docs for exact signatures;
//! this crate only supplies the row-shaped structs.
//!
//! ## New pattern (replaces SeaORM's Entity/Model/ActiveModel/Column split)
//!
//! Each table now gets ONE plain struct per module (module name = table name,
//! e.g. `user` -> `users` table), named `Model` for drop-in compatibility with
//! existing call sites that wrote `entity::user::Model`. There is no
//! `Entity`, `ActiveModel`, or `Column` type any more — callers write raw SQL
//! (`SELECT * FROM users WHERE ...`) and bind parameters with hiqlite's
//! `params!()` macro, then map results with `query_as::<user::Model, _>(...)`.
//! The `prelude` module re-exports every `Model` type under its old
//! `entity::prelude::X` name (e.g. `entity::prelude::User` is now an alias
//! for `entity::user::Model`) so downstream call sites that only used the
//! struct as a data holder need no changes; call sites that used
//! SeaORM-specific query builder methods (`Entity::find()`, `ActiveModel`,
//! relations, `Related<..>`) must be rewritten against raw SQL — that is out
//! of scope for this crate.
//!
//! ## Type mapping from the old Postgres/SeaORM types
//!
//! - `Uuid` (Postgres `UUID`) -> stored as `TEXT` (36-char hyphenated string).
//!   Fields keep the Rust type `uuid::Uuid` (with the `serde` feature
//!   enabled), which (de)serializes to/from that string representation
//!   automatically, so `query_as` "just works" against a TEXT column. When
//!   binding params for `execute`/`query_as`, pass `id.to_string()` (hiqlite's
//!   `params!()` macro does not special-case `Uuid` itself).
//! - `ChronoDateTimeUtc` (Postgres `TIMESTAMPTZ`) -> stored as `TEXT`
//!   (RFC 3339 / ISO 8601, e.g. `2026-07-10T12:00:00Z`). Fields keep the Rust
//!   type `chrono::DateTime<chrono::Utc>`, which (de)serializes to/from that
//!   string via `chrono`'s `serde` feature.
//! - `bool` (Postgres `BOOLEAN`) -> stored as SQLite `INTEGER` (0/1). Fields
//!   keep the Rust type `bool`; rusqlite (which hiqlite is built on) converts
//!   INTEGER 0/1 to/from `bool` natively.
//! - `Option<T>` -> unchanged; nullable column.
//! - `serde_json::Value` (Postgres `JSONB`, used for `webhooks.events` and
//!   `access_tokens.scopes`) -> stored as `TEXT` containing the serialized
//!   JSON document. Because a bare TEXT column round-trips through
//!   `query_as` as a plain `String` (not a nested JSON object), these fields
//!   are typed `String` here, holding the raw JSON text. Callers must
//!   `serde_json::from_str::<T>(&model.events)` to parse and
//!   `serde_json::to_string(&value)` to serialize before binding as a query
//!   param.
//! - `Vec<u8>` / Postgres `BYTEA` (`repo_secrets.encrypted_value`) -> stored
//!   as SQLite `BLOB`; the Rust type stays `Vec<u8>`.
//!
//! Struct field names are identical to the old SeaORM `Model` field names
//! (`id`, `username`, `owner_type`, `created_at`, ...) so downstream code
//! changes are mechanical.

pub mod access_token;
pub mod activity_event;
pub mod branch_protection_rule;
pub mod dev_workspace;
pub mod issue;
pub mod issue_comment;
pub mod issue_label;
pub mod label;
pub mod milestone;
pub mod notification;
pub mod org_member;
pub mod organization;
pub mod package;
pub mod pr_review;
pub mod pr_review_comment;
pub mod project;
pub mod project_card;
pub mod project_column;
pub mod pull_request;
pub mod repo_collaborator;
pub mod repo_mirror;
pub mod repo_secret;
pub mod repository;
pub mod ssh_key;
pub mod user;
pub mod webhook;
pub mod workflow_artifact;
pub mod workflow_job;
pub mod workflow_run;

/// Re-exports every table's `Model` struct under its old SeaORM
/// `entity::prelude::X` name, so `entity::prelude::User` etc. keep working
/// as plain type aliases (they are no longer SeaORM `Entity` types — see the
/// crate-level docs).
pub mod prelude {
    pub use super::access_token::Model as AccessToken;
    pub use super::activity_event::Model as ActivityEvent;
    pub use super::branch_protection_rule::Model as BranchProtectionRule;
    pub use super::dev_workspace::Model as DevWorkspace;
    pub use super::issue::Model as Issue;
    pub use super::issue_comment::Model as IssueComment;
    pub use super::issue_label::Model as IssueLabel;
    pub use super::label::Model as Label;
    pub use super::milestone::Model as Milestone;
    pub use super::notification::Model as Notification;
    pub use super::org_member::Model as OrgMember;
    pub use super::organization::Model as Organization;
    pub use super::package::Model as Package;
    pub use super::pr_review::Model as PrReview;
    pub use super::pr_review_comment::Model as PrReviewComment;
    pub use super::project::Model as Project;
    pub use super::project_card::Model as ProjectCard;
    pub use super::project_column::Model as ProjectColumn;
    pub use super::pull_request::Model as PullRequest;
    pub use super::repo_collaborator::Model as RepoCollaborator;
    pub use super::repo_mirror::Model as RepoMirror;
    pub use super::repo_secret::Model as RepoSecret;
    pub use super::repository::Model as Repository;
    pub use super::ssh_key::Model as SshKey;
    pub use super::user::Model as User;
    pub use super::webhook::Model as Webhook;
    pub use super::workflow_artifact::Model as WorkflowArtifact;
    pub use super::workflow_job::Model as WorkflowJob;
    pub use super::workflow_run::Model as WorkflowRun;
}

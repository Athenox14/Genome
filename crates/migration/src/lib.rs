//! Hiqlite-native migrations (replaces the old `sea-orm-migration` /
//! Postgres migrator).
//!
//! Hiqlite applies migrations via `hiqlite::Client::migrate::<T>()` where
//! `T` is a type deriving `rust_embed::Embed` (re-exported here as
//! `rust_embed::RustEmbed`) pointing at a folder of `N_name.sql` files
//! (1-indexed, gaps not required). Hiqlite tracks which numbered migrations
//! have already been applied (see `hiqlite::AppliedMigration`) and applies
//! any new ones, in order, on every call — so `migrate::<Migrations>()` is
//! safe to call unconditionally at startup.
//!
//! The ~15 incremental Postgres migrations that used to live in this crate
//! have been consolidated into a small number of logically-grouped SQLite
//! DDL files under `migrations/`, covering all ~29 tables in their final
//! shape directly (no more `ALTER TABLE ... ADD COLUMN` history to replay,
//! since this is a fresh SQLite schema).
//!
//! Usage from a server binary:
//!
//! ```ignore
//! let client = hiqlite::start_node(node_config).await?;
//! client.migrate::<migration::Migrations>().await?;
//! ```

#[derive(rust_embed::Embed)]
#[folder = "migrations"]
pub struct Migrations;

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, serde::Serialize, serde::Deserialize)]
#[sea_orm(table_name = "branch_protection_rules")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub repo_id: Uuid,
    pub branch_pattern: String,
    pub require_reviews_count: i32,
    pub require_status_checks: bool,
    pub block_force_push: bool,
    pub created_at: ChronoDateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::repository::Entity",
        from = "Column::RepoId",
        to = "super::repository::Column::Id"
    )]
    Repository,
}

impl Related<super::repository::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Repository.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}

/// Simple glob matching supporting a literal branch name (e.g. "main") or a
/// trailing wildcard segment (e.g. "release/*"). Not a full glob
/// implementation, but enough for common branch-protection patterns.
pub fn branch_matches_pattern(pattern: &str, branch: &str) -> bool {
    if pattern == branch {
        return true;
    }
    if let Some(prefix) = pattern.strip_suffix('*') {
        return branch.starts_with(prefix);
    }
    false
}

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, serde::Serialize, serde::Deserialize)]
#[sea_orm(table_name = "users")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    #[sea_orm(unique)]
    pub username: String,
    #[sea_orm(unique)]
    pub email: String,
    pub password_hash: String,
    #[sea_orm(default_value = false)]
    pub is_admin: bool,
    pub avatar_url: Option<String>,
    pub created_at: ChronoDateTimeUtc,
    pub totp_secret: Option<String>,
    #[sea_orm(default_value = false)]
    pub totp_enabled: bool,
    pub deactivated_at: Option<ChronoDateTimeUtc>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "super::ssh_key::Entity")]
    SshKey,
    #[sea_orm(has_many = "super::org_member::Entity")]
    OrgMember,
    #[sea_orm(has_many = "super::repo_collaborator::Entity")]
    RepoCollaborator,
    #[sea_orm(has_many = "super::issue::Entity")]
    Issue,
    #[sea_orm(has_many = "super::issue_comment::Entity")]
    IssueComment,
    #[sea_orm(has_many = "super::pull_request::Entity")]
    PullRequest,
    #[sea_orm(has_many = "super::pr_review::Entity")]
    PrReview,
    #[sea_orm(has_many = "super::dev_workspace::Entity")]
    DevWorkspace,
    #[sea_orm(has_many = "super::access_token::Entity")]
    AccessToken,
}

impl Related<super::ssh_key::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::SshKey.def()
    }
}
impl Related<super::org_member::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::OrgMember.def()
    }
}
impl Related<super::repo_collaborator::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::RepoCollaborator.def()
    }
}
impl Related<super::issue::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Issue.def()
    }
}
impl Related<super::issue_comment::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::IssueComment.def()
    }
}
impl Related<super::pull_request::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::PullRequest.def()
    }
}
impl Related<super::pr_review::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::PrReview.def()
    }
}
impl Related<super::dev_workspace::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::DevWorkspace.def()
    }
}
impl Related<super::access_token::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::AccessToken.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}

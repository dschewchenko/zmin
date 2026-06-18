use crate::domain::identifiers::{OrgId, RepoId};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RepoRole {
    Admin,
    Write,
    Read,
    Triager,
}

#[derive(Debug, Clone)]
pub struct Repository {
    pub id: RepoId,
    pub org_id: OrgId,
    pub name: String,
    pub default_branch: String,
    pub created_at: std::time::SystemTime,
}

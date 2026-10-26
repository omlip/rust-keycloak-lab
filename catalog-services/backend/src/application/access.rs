use crate::domain::selection::OwnerId;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Permission {
    CatalogRead,
    CatalogWrite,
    CatalogSeed,
    SelectionsRead,
    SelectionsWrite,
}

#[derive(Clone, Debug)]
pub struct Actor {
    owner: OwnerId,
    permissions: Vec<Permission>,
}

impl Actor {
    pub fn new(subject: impl Into<String>, permissions: &[Permission]) -> Self {
        Self {
            owner: OwnerId::new(subject),
            permissions: permissions.to_vec(),
        }
    }
    pub fn owner(&self) -> &OwnerId {
        &self.owner
    }
    pub fn permits(&self, permission: Permission) -> bool {
        self.permissions.contains(&permission)
    }
}

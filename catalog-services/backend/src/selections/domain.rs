use std::collections::BTreeSet;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OwnerId(String);

impl OwnerId {
    pub fn new(subject: impl Into<String>) -> Self {
        Self(subject.into())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SelectionError {
    PermissionDenied,
    NotFound,
    AccessDenied,
    InvalidName,
    ServiceNotFound,
    TotalOverflow,
    Conflict,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Selection {
    pub id: u64,
    name: String,
    owner: OwnerId,
    service_ids: BTreeSet<u64>,
}

impl Selection {
    pub fn new(id: u64, owner: OwnerId, name: &str) -> Result<Self, SelectionError> {
        let mut selection = Self {
            id,
            name: String::new(),
            owner,
            service_ids: BTreeSet::new(),
        };
        selection.rename(name)?;
        Ok(selection)
    }

    pub fn ensure_owner(&self, actor: &OwnerId) -> Result<(), SelectionError> {
        if &self.owner == actor {
            Ok(())
        } else {
            Err(SelectionError::AccessDenied)
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn owner(&self) -> &OwnerId {
        &self.owner
    }

    pub fn service_ids(&self) -> &BTreeSet<u64> {
        &self.service_ids
    }

    pub fn rename(&mut self, name: &str) -> Result<(), SelectionError> {
        let name = name.trim();
        if name.is_empty() {
            return Err(SelectionError::InvalidName);
        }
        self.name = name.to_owned();
        Ok(())
    }

    pub fn add_service(&mut self, id: u64) {
        self.service_ids.insert(id);
    }

    pub fn remove_service(&mut self, id: u64) {
        self.service_ids.remove(&id);
    }

    pub fn retain_services(&mut self, mut exists: impl FnMut(u64) -> bool) {
        self.service_ids.retain(|id| exists(*id));
    }
}

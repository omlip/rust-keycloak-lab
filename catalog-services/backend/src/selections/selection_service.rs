use super::{
    domain::{Selection, SelectionError},
    selection_repository::{SelectionRepository, SelectionSnapshot},
};
use crate::{
    access::{Actor, Permission},
    catalog::domain::DatacenterService,
};
use std::sync::Arc;

#[derive(Clone)]
pub struct SelectionService {
    repository: Arc<dyn SelectionRepository>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SelectionDetails {
    pub id: u64,
    pub name: String,
    pub services: Vec<DatacenterService>,
    pub monthly_total_cents: u64,
}

impl SelectionService {
    pub fn new(repository: Arc<dyn SelectionRepository>) -> Self {
        Self { repository }
    }

    pub fn list(&self, actor: &Actor) -> Result<Vec<SelectionDetails>, SelectionError> {
        Self::require(actor, Permission::SelectionsRead)?;
        self.repository
            .list()
            .into_iter()
            .filter(|s| s.selection.ensure_owner(actor.owner()).is_ok())
            .map(Self::details)
            .collect()
    }

    pub fn create(&self, actor: &Actor, name: &str) -> Result<SelectionDetails, SelectionError> {
        Self::require(actor, Permission::SelectionsWrite)?;
        let selection = Selection::new(0, actor.owner().clone(), name)?;
        Self::details(self.repository.create(selection))
    }

    fn owned(&self, actor: &Actor, id: u64) -> Result<SelectionSnapshot, SelectionError> {
        let snapshot = self.repository.find(id).ok_or(SelectionError::NotFound)?;
        // Business authorization lives here and in the domain, not in a storage adapter.
        snapshot.selection.ensure_owner(actor.owner())?;
        Ok(snapshot)
    }

    pub fn find(&self, actor: &Actor, id: u64) -> Result<SelectionDetails, SelectionError> {
        Self::require(actor, Permission::SelectionsRead)?;
        Self::details(self.owned(actor, id)?)
    }

    fn change(
        &self,
        actor: &Actor,
        id: u64,
        change: impl Fn(&mut SelectionSnapshot) -> Result<(), SelectionError>,
    ) -> Result<SelectionDetails, SelectionError> {
        Self::require(actor, Permission::SelectionsWrite)?;
        // Retry with a fresh snapshot AND repeat authorization after a concurrent write.
        for _ in 0..8 {
            let mut snapshot = self.owned(actor, id)?;
            change(&mut snapshot)?;
            Self::details(snapshot.clone())?; // Validate totals before persisting.
            if let Ok(saved) = self.repository.save(snapshot) {
                return Self::details(saved);
            }
        }
        Err(SelectionError::Conflict)
    }

    pub fn rename(
        &self,
        actor: &Actor,
        id: u64,
        name: &str,
    ) -> Result<SelectionDetails, SelectionError> {
        self.change(actor, id, |s| s.selection.rename(name))
    }

    pub fn add_service(
        &self,
        actor: &Actor,
        id: u64,
        service_id: u64,
    ) -> Result<SelectionDetails, SelectionError> {
        self.change(actor, id, |s| {
            if !s.catalog.iter().any(|service| service.id == service_id) {
                return Err(SelectionError::ServiceNotFound);
            }
            s.selection.add_service(service_id);
            Ok(())
        })
    }

    pub fn remove_service(
        &self,
        actor: &Actor,
        id: u64,
        service_id: u64,
    ) -> Result<SelectionDetails, SelectionError> {
        self.change(actor, id, |s| {
            s.selection.remove_service(service_id);
            Ok(())
        })
    }

    pub fn delete(&self, actor: &Actor, id: u64) -> Result<(), SelectionError> {
        Self::require(actor, Permission::SelectionsWrite)?;
        for _ in 0..8 {
            let snapshot = self.owned(actor, id)?;
            if self.repository.delete(id, snapshot.version).is_ok() {
                return Ok(());
            }
        }
        Err(SelectionError::Conflict)
    }

    fn require(actor: &Actor, permission: Permission) -> Result<(), SelectionError> {
        if actor.permits(permission) {
            Ok(())
        } else {
            Err(SelectionError::PermissionDenied)
        }
    }

    fn details(snapshot: SelectionSnapshot) -> Result<SelectionDetails, SelectionError> {
        let services: Vec<_> = snapshot
            .catalog
            .into_iter()
            .filter(|s| snapshot.selection.service_ids().contains(&s.id))
            .collect();
        let monthly_total_cents = services.iter().try_fold(0u64, |total, s| {
            total
                .checked_add(s.data.monthly_price_cents)
                .ok_or(SelectionError::TotalOverflow)
        })?;
        Ok(SelectionDetails {
            id: snapshot.selection.id,
            name: snapshot.selection.name().to_owned(),
            services,
            monthly_total_cents,
        })
    }
}

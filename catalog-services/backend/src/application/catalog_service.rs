use crate::{
    application::{
        access::{Actor, Permission},
        ports::catalog_repository::CatalogRepository,
    },
    domain::catalog::{DatacenterService, ServiceDraft},
};
use std::sync::Arc;

#[derive(Clone)]
pub struct CatalogService {
    repository: Arc<dyn CatalogRepository>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum CatalogError {
    ServiceNotFound,
    PermissionDenied,
}

impl CatalogService {
    pub fn new(repository: Arc<dyn CatalogRepository>) -> Self {
        Self { repository }
    }

    pub fn list(&self, actor: &Actor) -> Result<Vec<DatacenterService>, CatalogError> {
        Self::require(actor, Permission::CatalogRead)?;
        Ok(self.repository.list())
    }

    pub fn find(&self, actor: &Actor, id: u64) -> Result<DatacenterService, CatalogError> {
        Self::require(actor, Permission::CatalogRead)?;
        self.repository
            .find(id)
            .ok_or(CatalogError::ServiceNotFound)
    }

    pub fn create(
        &self,
        actor: &Actor,
        data: ServiceDraft,
    ) -> Result<DatacenterService, CatalogError> {
        Self::require(actor, Permission::CatalogWrite)?;
        Ok(self.repository.create(data))
    }

    pub fn update(
        &self,
        actor: &Actor,
        id: u64,
        data: ServiceDraft,
    ) -> Result<DatacenterService, CatalogError> {
        Self::require(actor, Permission::CatalogWrite)?;
        self.repository
            .update(id, data)
            .ok_or(CatalogError::ServiceNotFound)
    }

    pub fn delete(&self, actor: &Actor, id: u64) -> Result<(), CatalogError> {
        Self::require(actor, Permission::CatalogWrite)?;
        if self.repository.delete(id) {
            Ok(())
        } else {
            Err(CatalogError::ServiceNotFound)
        }
    }

    pub fn replace_all(
        &self,
        actor: &Actor,
        services: Vec<ServiceDraft>,
    ) -> Result<usize, CatalogError> {
        Self::require(actor, Permission::CatalogSeed)?;
        Ok(self.repository.replace_all(services))
    }

    fn require(actor: &Actor, permission: Permission) -> Result<(), CatalogError> {
        if actor.permits(permission) {
            Ok(())
        } else {
            Err(CatalogError::PermissionDenied)
        }
    }
}

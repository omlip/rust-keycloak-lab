use super::{
    catalog_repository::CatalogRepository,
    domain::{DatacenterService, ServiceDraft},
};
use std::sync::Arc;

#[derive(Clone)]
pub struct CatalogService {
    repository: Arc<dyn CatalogRepository>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum CatalogError {
    ServiceNotFound,
}

impl CatalogService {
    pub fn new(repository: Arc<dyn CatalogRepository>) -> Self {
        Self { repository }
    }

    pub fn list(&self) -> Vec<DatacenterService> {
        self.repository.list()
    }

    pub fn find(&self, id: u64) -> Result<DatacenterService, CatalogError> {
        self.repository
            .find(id)
            .ok_or(CatalogError::ServiceNotFound)
    }

    pub fn create(&self, data: ServiceDraft) -> DatacenterService {
        self.repository.create(data)
    }

    pub fn update(&self, id: u64, data: ServiceDraft) -> Result<DatacenterService, CatalogError> {
        self.repository
            .update(id, data)
            .ok_or(CatalogError::ServiceNotFound)
    }

    pub fn delete(&self, id: u64) -> Result<(), CatalogError> {
        if self.repository.delete(id) {
            Ok(())
        } else {
            Err(CatalogError::ServiceNotFound)
        }
    }

    pub fn replace_all(&self, services: Vec<ServiceDraft>) -> usize {
        self.repository.replace_all(services)
    }
}

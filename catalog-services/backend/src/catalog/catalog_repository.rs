use super::domain::{DatacenterService, ServiceDraft};

// Outgoing port: no HTTP, JWT or storage implementation leaks into this contract.
pub trait CatalogRepository: Send + Sync {
    fn list(&self) -> Vec<DatacenterService>;
    fn find(&self, id: u64) -> Option<DatacenterService>;
    fn create(&self, data: ServiceDraft) -> DatacenterService;
    fn update(&self, id: u64, data: ServiceDraft) -> Option<DatacenterService>;
    fn delete(&self, id: u64) -> bool;
    fn replace_all(&self, services: Vec<ServiceDraft>) -> usize;
}

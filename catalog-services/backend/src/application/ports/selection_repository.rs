use crate::domain::{catalog::DatacenterService, selection::Selection};

// A coherent read, independent of HTTP/JWT and of a specific storage engine.
#[derive(Clone)]
pub struct SelectionSnapshot {
    pub selection: Selection,
    pub version: u64,
    pub catalog_version: u64,
    pub catalog: Vec<DatacenterService>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum WriteError {
    Conflict,
}

pub trait SelectionRepository: Send + Sync {
    fn list(&self) -> Vec<SelectionSnapshot>;
    fn find(&self, id: u64) -> Option<SelectionSnapshot>;
    // Storage assigns the identifier. The supplied selection has already been validated.
    fn create(&self, selection: Selection) -> SelectionSnapshot;
    // Compare-and-swap: reject stale selection OR catalog versions, atomically.
    // The owner is immutable. This contract must also hold for a future DB adapter.
    fn save(&self, snapshot: SelectionSnapshot) -> Result<SelectionSnapshot, WriteError>;
    fn delete(&self, id: u64, expected_version: u64) -> Result<(), WriteError>;
}

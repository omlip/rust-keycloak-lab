use super::{
    catalog_repository::CatalogRepository,
    domain::{DatacenterService, ServiceDraft},
};
use std::{collections::BTreeMap, sync::Mutex};

pub struct InMemoryCatalogRepository {
    state: Mutex<CatalogState>,
}

struct CatalogState {
    services: BTreeMap<u64, DatacenterService>,
    next_id: u64,
}

impl Default for InMemoryCatalogRepository {
    fn default() -> Self {
        Self {
            state: Mutex::new(CatalogState {
                services: BTreeMap::new(),
                next_id: 1,
            }),
        }
    }
}

impl CatalogState {
    fn insert(&mut self, data: ServiceDraft) -> DatacenterService {
        let service = DatacenterService {
            id: self.next_id,
            data,
        };
        self.next_id += 1;
        self.services.insert(service.id, service.clone());
        service
    }
}

impl CatalogRepository for InMemoryCatalogRepository {
    fn list(&self) -> Vec<DatacenterService> {
        self.state
            .lock()
            .expect("catalog lock poisoned")
            .services
            .values()
            .cloned()
            .collect()
    }

    fn find(&self, id: u64) -> Option<DatacenterService> {
        self.state
            .lock()
            .expect("catalog lock poisoned")
            .services
            .get(&id)
            .cloned()
    }

    fn create(&self, data: ServiceDraft) -> DatacenterService {
        self.state
            .lock()
            .expect("catalog lock poisoned")
            .insert(data)
    }

    fn update(&self, id: u64, data: ServiceDraft) -> Option<DatacenterService> {
        let mut state = self.state.lock().expect("catalog lock poisoned");
        let service = state.services.get_mut(&id)?;
        service.data = data;
        Some(service.clone())
    }

    fn delete(&self, id: u64) -> bool {
        self.state
            .lock()
            .expect("catalog lock poisoned")
            .services
            .remove(&id)
            .is_some()
    }

    fn replace_all(&self, services: Vec<ServiceDraft>) -> usize {
        let mut state = self.state.lock().expect("catalog lock poisoned");
        // One critical section: readers never observe a partially replaced catalogue.
        // Keep IDs increasing so stale clients cannot edit a newly seeded service by accident.
        state.services.clear();
        for service in services {
            state.insert(service);
        }
        state.services.len()
    }
}

use crate::{
    application::ports::{
        catalog_repository::CatalogRepository,
        selection_repository::{SelectionRepository, SelectionSnapshot, WriteError},
    },
    domain::{
        catalog::{DatacenterService, ServiceDraft},
        selection::Selection,
    },
};
use std::{collections::BTreeMap, sync::Mutex};

pub struct InMemoryStore {
    state: Mutex<StoreState>,
}

struct StoreState {
    services: BTreeMap<u64, DatacenterService>,
    next_id: u64,
    catalog_version: u64,
    selections: BTreeMap<u64, (Selection, u64)>,
    next_selection_id: u64,
}

impl Default for InMemoryStore {
    fn default() -> Self {
        Self {
            state: Mutex::new(StoreState {
                services: BTreeMap::new(),
                next_id: 1,
                catalog_version: 0,
                selections: BTreeMap::new(),
                next_selection_id: 1,
            }),
        }
    }
}

impl StoreState {
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

impl CatalogRepository for InMemoryStore {
    fn list(&self) -> Vec<DatacenterService> {
        self.state
            .lock()
            .expect("store lock poisoned")
            .services
            .values()
            .cloned()
            .collect()
    }

    fn find(&self, id: u64) -> Option<DatacenterService> {
        self.state
            .lock()
            .expect("store lock poisoned")
            .services
            .get(&id)
            .cloned()
    }

    fn create(&self, data: ServiceDraft) -> DatacenterService {
        let mut state = self.state.lock().expect("store lock poisoned");
        state.catalog_version += 1;
        state.insert(data)
    }

    fn update(&self, id: u64, data: ServiceDraft) -> Option<DatacenterService> {
        let mut state = self.state.lock().expect("store lock poisoned");
        if !state.services.contains_key(&id) {
            return None;
        }
        state.catalog_version += 1;
        let service = state.services.get_mut(&id)?;
        service.data = data;
        Some(service.clone())
    }

    fn delete(&self, id: u64) -> bool {
        let mut state = self.state.lock().expect("store lock poisoned");
        if state.services.remove(&id).is_none() {
            return false;
        }
        state.catalog_version += 1;
        for (selection, version) in state.selections.values_mut() {
            if selection.service_ids().contains(&id) {
                selection.remove_service(id);
                *version += 1;
            }
        }
        true
    }

    fn replace_all(&self, services: Vec<ServiceDraft>) -> usize {
        let mut state = self.state.lock().expect("store lock poisoned");
        // One critical section: readers never observe a partially replaced catalogue.
        // Keep IDs increasing so stale clients cannot edit a newly seeded service by accident.
        state.services.clear();
        state.catalog_version += 1;
        for (selection, version) in state.selections.values_mut() {
            selection.retain_services(|_| false);
            *version += 1;
        }
        for service in services {
            state.insert(service);
        }
        state.services.len()
    }
}

impl StoreState {
    fn snapshot(&self, selection: &Selection, version: u64) -> SelectionSnapshot {
        SelectionSnapshot {
            selection: selection.clone(),
            version,
            catalog_version: self.catalog_version,
            catalog: self.services.values().cloned().collect(),
        }
    }
}

impl SelectionRepository for InMemoryStore {
    fn list(&self) -> Vec<SelectionSnapshot> {
        let state = self.state.lock().expect("store lock poisoned");
        state
            .selections
            .values()
            .map(|(s, v)| state.snapshot(s, *v))
            .collect()
    }

    fn find(&self, id: u64) -> Option<SelectionSnapshot> {
        let state = self.state.lock().expect("store lock poisoned");
        state
            .selections
            .get(&id)
            .map(|(s, v)| state.snapshot(s, *v))
    }

    fn create(&self, mut selection: Selection) -> SelectionSnapshot {
        let mut state = self.state.lock().expect("store lock poisoned");
        selection.id = state.next_selection_id;
        state.next_selection_id += 1;
        state
            .selections
            .insert(selection.id, (selection.clone(), 0));
        state.snapshot(&selection, 0)
    }

    fn save(&self, snapshot: SelectionSnapshot) -> Result<SelectionSnapshot, WriteError> {
        let mut state = self.state.lock().expect("store lock poisoned");
        let Some((current, version)) = state.selections.get(&snapshot.selection.id) else {
            return Err(WriteError::Conflict);
        };
        if *version != snapshot.version || state.catalog_version != snapshot.catalog_version {
            return Err(WriteError::Conflict);
        }
        if current.owner() != snapshot.selection.owner() {
            return Err(WriteError::Conflict);
        }
        // Storage enforces referential integrity. Authorization was checked in the service.
        // Ownership cannot be changed: Selection exposes no owner mutation method.
        if snapshot
            .selection
            .service_ids()
            .iter()
            .any(|id| !state.services.contains_key(id))
        {
            return Err(WriteError::Conflict);
        }
        let id = current.id;
        let version = version + 1;
        state
            .selections
            .insert(id, (snapshot.selection.clone(), version));
        Ok(state.snapshot(&snapshot.selection, version))
    }

    fn delete(&self, id: u64, expected_version: u64) -> Result<(), WriteError> {
        let mut state = self.state.lock().expect("store lock poisoned");
        if !state
            .selections
            .get(&id)
            .is_some_and(|(_, v)| *v == expected_version)
        {
            return Err(WriteError::Conflict);
        }
        state.selections.remove(&id);
        Ok(())
    }
}

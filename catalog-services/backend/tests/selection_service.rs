use olaninc_api::{
    application::{
        access::{Actor, Permission},
        ports::{
            catalog_repository::CatalogRepository,
            selection_repository::{SelectionRepository, SelectionSnapshot, WriteError},
        },
        selection_service::SelectionService,
    },
    domain::{
        catalog::{Category, ServiceDraft},
        selection::{OwnerId, Selection, SelectionError},
    },
    infrastructure::persistence::in_memory_store::InMemoryStore,
};
use std::sync::{Arc, Barrier};

fn actor(name: &str) -> Actor {
    Actor::new(
        name,
        &[Permission::SelectionsRead, Permission::SelectionsWrite],
    )
}

fn draft(price: u64) -> ServiceDraft {
    ServiceDraft {
        name: "VM".into(),
        category: Category::Compute,
        description: String::new(),
        monthly_price_cents: price,
    }
}

fn setup() -> (
    Arc<InMemoryStore>,
    Arc<dyn CatalogRepository>,
    SelectionService,
    Actor,
) {
    let store = Arc::new(InMemoryStore::default());
    let catalog: Arc<dyn CatalogRepository> = store.clone();
    let selections = SelectionService::new(store.clone());
    (store, catalog, selections, actor("alice"))
}

#[test]
fn domain_owns_authorization_and_name_validation() {
    let alice = OwnerId::new("alice");
    let mut selection = Selection::new(1, alice.clone(), "  Montréal  ").unwrap();
    assert_eq!(selection.name(), "Montréal");
    assert_eq!(selection.ensure_owner(&alice), Ok(()));
    assert_eq!(
        selection.ensure_owner(&OwnerId::new("bob")),
        Err(SelectionError::AccessDenied)
    );
    assert_eq!(selection.rename(" \n "), Err(SelectionError::InvalidName));
    assert_eq!(selection.name(), "Montréal");
}

// This adapter deliberately returns foreign data without filtering. The service
// must enforce the business policy before returning data or requesting a write.
struct UnfilteredRepository(SelectionSnapshot);
impl SelectionRepository for UnfilteredRepository {
    fn list(&self) -> Vec<SelectionSnapshot> {
        vec![self.0.clone()]
    }
    fn find(&self, _: u64) -> Option<SelectionSnapshot> {
        Some(self.0.clone())
    }
    fn create(&self, _: Selection) -> SelectionSnapshot {
        panic!("unexpected create")
    }
    fn save(&self, _: SelectionSnapshot) -> Result<SelectionSnapshot, WriteError> {
        panic!("unauthorized save")
    }
    fn delete(&self, _: u64, _: u64) -> Result<(), WriteError> {
        panic!("unauthorized delete")
    }
}

#[test]
fn service_protects_every_operation_even_with_an_unfiltered_adapter() {
    let snapshot = SelectionSnapshot {
        selection: Selection::new(1, OwnerId::new("alice"), "Privée").unwrap(),
        version: 0,
        catalog_version: 0,
        catalog: vec![],
    };
    let service = SelectionService::new(Arc::new(UnfilteredRepository(snapshot)));
    let bob = actor("bob");
    assert_eq!(service.list(&bob).unwrap(), vec![]);
    assert_eq!(service.find(&bob, 1), Err(SelectionError::AccessDenied));
    assert_eq!(
        service.rename(&bob, 1, "Attaque"),
        Err(SelectionError::AccessDenied)
    );
    assert_eq!(
        service.add_service(&bob, 1, 999),
        Err(SelectionError::AccessDenied)
    );
    assert_eq!(
        service.remove_service(&bob, 1, 999),
        Err(SelectionError::AccessDenied)
    );
    assert_eq!(service.delete(&bob, 1), Err(SelectionError::AccessDenied));
}

#[test]
fn personal_crud_totals_and_duplicate_free_membership() {
    let (_, catalog, selections, alice) = setup();
    let vm = catalog.create(draft(1250));
    let disk = catalog.create(draft(500));
    assert!(selections.list(&alice).unwrap().is_empty());
    assert_eq!(
        selections.create(&alice, "  "),
        Err(SelectionError::InvalidName)
    );
    let first = selections.create(&alice, "Montréal").unwrap();
    let second = selections.create(&alice, "Tests").unwrap();
    assert_ne!(first.id, second.id);
    assert_eq!(first.monthly_total_cents, 0);
    selections.add_service(&alice, first.id, vm.id).unwrap();
    selections.add_service(&alice, first.id, vm.id).unwrap();
    let details = selections.add_service(&alice, first.id, disk.id).unwrap();
    assert_eq!(details.services.len(), 2);
    assert_eq!(details.monthly_total_cents, 1750);
    assert_eq!(
        selections.add_service(&alice, first.id, 999),
        Err(SelectionError::ServiceNotFound)
    );
    assert_eq!(
        selections
            .rename(&alice, first.id, " Production ")
            .unwrap()
            .name,
        "Production"
    );
    assert_eq!(
        selections
            .remove_service(&alice, first.id, disk.id)
            .unwrap()
            .monthly_total_cents,
        1250
    );
    selections
        .remove_service(&alice, first.id, disk.id)
        .unwrap();
    selections.delete(&alice, first.id).unwrap();
    assert_eq!(
        selections.find(&alice, first.id),
        Err(SelectionError::NotFound)
    );
    assert_eq!(catalog.list().len(), 2);
    assert_eq!(selections.list(&alice).unwrap().len(), 1);
}

#[test]
fn current_prices_deletion_and_seed_are_shared_across_owners() {
    let (store, catalog, selections, alice) = setup();
    let bob = actor("bob");
    let vm = catalog.create(draft(1000));
    let a = selections.create(&alice, "A").unwrap();
    let b = selections.create(&bob, "B").unwrap();
    for (owner, id) in [(&alice, a.id), (&bob, b.id)] {
        selections.add_service(owner, id, vm.id).unwrap();
    }
    catalog.update(vm.id, draft(2500)).unwrap();
    assert_eq!(
        selections.find(&alice, a.id).unwrap().monthly_total_cents,
        2500
    );
    assert!(catalog.delete(vm.id));
    assert!(
        SelectionRepository::find(store.as_ref(), a.id)
            .unwrap()
            .selection
            .service_ids()
            .is_empty()
    );
    assert_eq!(selections.find(&bob, b.id).unwrap().monthly_total_cents, 0);
    let other = catalog.create(draft(700));
    selections.add_service(&alice, a.id, other.id).unwrap();
    catalog.replace_all(vec![draft(400)]);
    let details = selections.find(&alice, a.id).unwrap();
    assert_eq!(details.name, "A");
    assert!(details.services.is_empty());
    assert!(
        SelectionRepository::find(store.as_ref(), a.id)
            .unwrap()
            .selection
            .service_ids()
            .is_empty()
    );
    let fresh = SelectionService::new(Arc::new(InMemoryStore::default()));
    assert!(fresh.list(&alice).unwrap().is_empty());
}

#[test]
fn stale_writes_cannot_resurrect_deleted_services_or_overwrite_selections() {
    let (store, catalog, selections, alice) = setup();
    let vm = catalog.create(draft(1000));
    let selection = selections.create(&alice, "A").unwrap();
    let mut stale = SelectionRepository::find(store.as_ref(), selection.id).unwrap();
    stale.selection.add_service(vm.id);
    assert!(catalog.delete(vm.id));
    assert!(matches!(store.save(stale), Err(WriteError::Conflict)));
    let mut stale = SelectionRepository::find(store.as_ref(), selection.id).unwrap();
    selections.rename(&alice, selection.id, "B").unwrap();
    stale.selection.rename("C").unwrap();
    assert!(matches!(
        store.save(stale.clone()),
        Err(WriteError::Conflict)
    ));
    assert_eq!(
        SelectionRepository::delete(store.as_ref(), selection.id, stale.version),
        Err(WriteError::Conflict)
    );
    assert_eq!(selections.find(&alice, selection.id).unwrap().name, "B");
}

#[test]
fn totals_never_wrap_and_failed_addition_is_not_persisted() {
    let (_, catalog, selections, alice) = setup();
    let big = catalog.create(draft(u64::MAX));
    let small = catalog.create(draft(1));
    let selection = selections.create(&alice, "A").unwrap();
    selections
        .add_service(&alice, selection.id, big.id)
        .unwrap();
    assert_eq!(
        selections.add_service(&alice, selection.id, small.id),
        Err(SelectionError::TotalOverflow)
    );
    assert_eq!(
        selections
            .find(&alice, selection.id)
            .unwrap()
            .services
            .len(),
        1
    );
}

#[test]
fn concurrent_additions_do_not_lose_updates() {
    let (_, catalog, selections, alice) = setup();
    let ids: Vec<_> = (0..4).map(|_| catalog.create(draft(100)).id).collect();
    let id = selections.create(&alice, "A").unwrap().id;
    let barrier = Arc::new(Barrier::new(ids.len()));
    let threads: Vec<_> = ids
        .into_iter()
        .map(|service_id| {
            let selections = selections.clone();
            let alice = alice.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                selections.add_service(&alice, id, service_id).unwrap();
            })
        })
        .collect();
    for thread in threads {
        thread.join().unwrap();
    }
    let result = selections.find(&alice, id).unwrap();
    assert_eq!(result.services.len(), 4);
    assert_eq!(result.monthly_total_cents, 400);
}

#[test]
fn a_read_only_actor_cannot_mutate_even_when_calling_the_application_service_directly() {
    let (_, catalog, selections, alice) = setup();
    let selected = selections.create(&alice, "Private").unwrap();
    let service = catalog.create(draft(100));
    let reader = Actor::new("alice", &[Permission::SelectionsRead]);
    assert!(selections.find(&reader, selected.id).is_ok());
    assert_eq!(
        selections.create(&reader, "No"),
        Err(SelectionError::PermissionDenied)
    );
    assert_eq!(
        selections.rename(&reader, selected.id, "No"),
        Err(SelectionError::PermissionDenied)
    );
    assert_eq!(
        selections.delete(&reader, selected.id),
        Err(SelectionError::PermissionDenied)
    );
    assert_eq!(
        selections.add_service(&reader, selected.id, service.id),
        Err(SelectionError::PermissionDenied)
    );
    assert_eq!(
        selections.remove_service(&reader, selected.id, service.id),
        Err(SelectionError::PermissionDenied)
    );
}

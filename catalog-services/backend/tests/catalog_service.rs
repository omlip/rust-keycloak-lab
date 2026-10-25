use olaninc_api::access::{Actor, Permission};
use olaninc_api::catalog::{
    catalog_service::{CatalogError, CatalogService},
    domain::{Category, ServiceDraft},
};
use olaninc_api::storage::in_memory_store::InMemoryStore;
use std::sync::Arc;

fn actor() -> Actor {
    Actor::new(
        "admin",
        &[
            Permission::CatalogRead,
            Permission::CatalogWrite,
            Permission::CatalogSeed,
        ],
    )
}

fn draft(name: &str) -> ServiceDraft {
    ServiceDraft {
        name: name.into(),
        category: Category::Compute,
        description: String::new(),
        monthly_price_cents: 1000,
    }
}

#[test]
fn application_supports_crud_without_http_or_jwt() {
    let catalog = CatalogService::new(Arc::new(InMemoryStore::default()));
    assert!(catalog.list(&actor()).unwrap().is_empty());
    let created = catalog.create(&actor(), draft("VM")).unwrap();
    assert_eq!(catalog.find(&actor(), created.id), Ok(created.clone()));
    let updated = catalog
        .update(&actor(), created.id, draft("Serveur dédié"))
        .unwrap();
    assert_eq!(updated.id, created.id);
    assert_eq!(updated.data.name, "Serveur dédié");
    assert_eq!(catalog.list(&actor()).unwrap(), vec![updated]);
    catalog.delete(&actor(), created.id).unwrap();
    assert_eq!(
        catalog.find(&actor(), created.id),
        Err(CatalogError::ServiceNotFound)
    );
    assert_eq!(
        catalog.update(&actor(), created.id, draft("Absent")),
        Err(CatalogError::ServiceNotFound)
    );
    assert_eq!(
        catalog.delete(&actor(), created.id),
        Err(CatalogError::ServiceNotFound)
    );
}

#[test]
fn replacement_is_shared_and_new_repository_is_empty() {
    let catalog = CatalogService::new(Arc::new(InMemoryStore::default()));
    let old = catalog.create(&actor(), draft("Ancien")).unwrap();
    let other_client = catalog.clone();
    assert_eq!(
        catalog
            .replace_all(&actor(), vec![draft("A"), draft("B")])
            .unwrap(),
        2
    );
    assert_eq!(other_client.list(&actor()).unwrap().len(), 2);
    assert_eq!(
        catalog.find(&actor(), old.id),
        Err(CatalogError::ServiceNotFound)
    );
    assert!(
        other_client
            .list(&actor())
            .unwrap()
            .iter()
            .all(|service| service.id > old.id)
    );
    assert_eq!(catalog.replace_all(&actor(), vec![]).unwrap(), 0);
    let new_catalog = CatalogService::new(Arc::new(InMemoryStore::default()));
    assert!(new_catalog.list(&actor()).unwrap().is_empty());
}

#[test]
fn concurrent_creates_have_distinct_ids() {
    let catalog = CatalogService::new(Arc::new(InMemoryStore::default()));
    let threads: Vec<_> = (0..8)
        .map(|_| {
            let catalog = catalog.clone();
            std::thread::spawn(move || {
                for _ in 0..20 {
                    catalog.create(&actor(), draft("VM")).unwrap();
                }
            })
        })
        .collect();
    for thread in threads {
        thread.join().unwrap();
    }
    let entries = catalog.list(&actor()).unwrap();
    assert_eq!(entries.len(), 160);
    let mut ids: Vec<_> = entries.iter().map(|entry| entry.id).collect();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), 160);
}

#[test]
fn delegated_read_only_actor_cannot_use_catalog_application_operations() {
    let catalog = CatalogService::new(Arc::new(InMemoryStore::default()));
    let reader = Actor::new("admin", &[Permission::SelectionsRead]);
    assert_eq!(catalog.list(&reader), Err(CatalogError::PermissionDenied));
    assert_eq!(
        catalog.create(&reader, draft("VM")),
        Err(CatalogError::PermissionDenied)
    );
    assert_eq!(
        catalog.update(&reader, 1, draft("VM")),
        Err(CatalogError::PermissionDenied)
    );
    assert_eq!(
        catalog.delete(&reader, 1),
        Err(CatalogError::PermissionDenied)
    );
    assert_eq!(
        catalog.replace_all(&reader, vec![]),
        Err(CatalogError::PermissionDenied)
    );
}

use olaninc_api::catalog::{
    catalog_service::{CatalogError, CatalogService},
    domain::{Category, ServiceDraft},
    in_memory_repository::InMemoryCatalogRepository,
};
use std::sync::Arc;

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
    let catalog = CatalogService::new(Arc::new(InMemoryCatalogRepository::default()));
    assert!(catalog.list().is_empty());
    let created = catalog.create(draft("VM"));
    assert_eq!(catalog.find(created.id), Ok(created.clone()));
    let updated = catalog.update(created.id, draft("Serveur dédié")).unwrap();
    assert_eq!(updated.id, created.id);
    assert_eq!(updated.data.name, "Serveur dédié");
    assert_eq!(catalog.list(), vec![updated]);
    catalog.delete(created.id).unwrap();
    assert_eq!(catalog.find(created.id), Err(CatalogError::ServiceNotFound));
    assert_eq!(
        catalog.update(created.id, draft("Absent")),
        Err(CatalogError::ServiceNotFound)
    );
    assert_eq!(
        catalog.delete(created.id),
        Err(CatalogError::ServiceNotFound)
    );
}

#[test]
fn replacement_is_shared_and_new_repository_is_empty() {
    let catalog = CatalogService::new(Arc::new(InMemoryCatalogRepository::default()));
    let old = catalog.create(draft("Ancien"));
    let other_client = catalog.clone();
    assert_eq!(catalog.replace_all(vec![draft("A"), draft("B")]), 2);
    assert_eq!(other_client.list().len(), 2);
    assert_eq!(catalog.find(old.id), Err(CatalogError::ServiceNotFound));
    assert!(
        other_client
            .list()
            .iter()
            .all(|service| service.id > old.id)
    );
    assert_eq!(catalog.replace_all(vec![]), 0);
    let new_catalog = CatalogService::new(Arc::new(InMemoryCatalogRepository::default()));
    assert!(new_catalog.list().is_empty());
}

#[test]
fn concurrent_creates_have_distinct_ids() {
    let catalog = CatalogService::new(Arc::new(InMemoryCatalogRepository::default()));
    let threads: Vec<_> = (0..8)
        .map(|_| {
            let catalog = catalog.clone();
            std::thread::spawn(move || {
                for _ in 0..20 {
                    catalog.create(draft("VM"));
                }
            })
        })
        .collect();
    for thread in threads {
        thread.join().unwrap();
    }
    let entries = catalog.list();
    assert_eq!(entries.len(), 160);
    let mut ids: Vec<_> = entries.iter().map(|entry| entry.id).collect();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), 160);
}

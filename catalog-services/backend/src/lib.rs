pub mod access;
pub mod auth;
pub mod catalog;
pub mod selections;
pub mod storage;

use auth::{Authenticator, authenticate};
use axum::{Json, Router, middleware, routing::get};
use catalog::{catalog_http, catalog_service::CatalogService};
use selections::{selection_http, selection_service::SelectionService};
use serde::Serialize;
use std::sync::Arc;
use storage::in_memory_store::InMemoryStore;

#[derive(Serialize)]
struct Message {
    message: &'static str,
}

pub fn app(auth: Authenticator) -> Router {
    let store = Arc::new(InMemoryStore::default());
    let catalog = CatalogService::new(store.clone());
    let selections = SelectionService::new(store);
    app_with_services(auth, catalog, selections)
}

pub fn app_with_services(
    auth: Authenticator,
    catalog: CatalogService,
    selections: SelectionService,
) -> Router {
    let protected = Router::new()
        .merge(catalog_http::routes(catalog))
        .merge(selection_http::routes(selections))
        .route_layer(middleware::from_fn_with_state(auth, authenticate));

    Router::new()
        .route("/health", get(|| async { Json(Message { message: "ok" }) }))
        .merge(protected)
}

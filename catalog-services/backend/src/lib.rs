pub mod application;
pub mod domain;
pub mod infrastructure;

use application::{catalog_service::CatalogService, selection_service::SelectionService};
use axum::{Json, Router, middleware, routing::get};
use infrastructure::{
    auth::{Authenticator, authenticate},
    http::{catalog_http, selection_http},
    persistence::in_memory_store::InMemoryStore,
};
use serde::Serialize;
use std::sync::Arc;

#[derive(Serialize)]
struct Message {
    message: &'static str,
}

/// Composition root: choose the concrete adapters and inject them into use cases.
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

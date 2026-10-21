pub mod auth;
pub mod catalog;

use auth::{ApiError, Authenticator, Claims, authenticate};
use axum::{Extension, Json, Router, middleware, routing::get};
use catalog::{
    catalog_http, catalog_service::CatalogService, in_memory_repository::InMemoryCatalogRepository,
};
use serde::Serialize;
use std::sync::Arc;

#[derive(Serialize)]
struct Message {
    message: &'static str,
}

pub fn app(auth: Authenticator) -> Router {
    let catalog = CatalogService::new(Arc::new(InMemoryCatalogRepository::default()));
    app_with_catalog(auth, catalog)
}

pub fn app_with_catalog(auth: Authenticator, catalog: CatalogService) -> Router {
    let protected = Router::new()
        .route("/api/user", get(user))
        .route("/api/admin", get(admin))
        .merge(catalog_http::routes(catalog))
        .route_layer(middleware::from_fn_with_state(auth, authenticate));

    Router::new()
        .route("/health", get(|| async { Json(Message { message: "ok" }) }))
        .merge(protected)
}

async fn user(Extension(claims): Extension<Claims>) -> Result<Json<Message>, ApiError> {
    claims.require_any_role(&["users", "admin"])?;
    Ok(Json(Message {
        message: "Bienvenue dans l’espace utilisateur",
    }))
}

async fn admin(Extension(claims): Extension<Claims>) -> Result<Json<Message>, ApiError> {
    claims.require_any_role(&["admin"])?;
    Ok(Json(Message {
        message: "Bienvenue dans l’espace administration",
    }))
}

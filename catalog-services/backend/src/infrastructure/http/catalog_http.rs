use crate::{
    application::catalog_service::{CatalogError, CatalogService},
    domain::catalog::{Category, DatacenterService, ServiceDraft},
    infrastructure::{
        auth::{ApiError, Claims},
        seed::demo_catalog,
    },
};
use axum::{
    Extension, Json, Router,
    extract::{Path, State, rejection::JsonRejection},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};

pub fn routes(catalog: CatalogService) -> Router {
    Router::new()
        .route("/api/catalog/services", get(list).post(create))
        .route(
            "/api/catalog/services/{id}",
            get(find).put(update).delete(delete),
        )
        .route("/api/catalog/seed", post(seed))
        .with_state(catalog)
}

#[derive(Deserialize)]
struct ServiceInput {
    name: String,
    category: String,
    description: String,
    monthly_price_cents: u64,
}

impl ServiceInput {
    fn into_draft(self) -> Result<ServiceDraft, HttpError> {
        Ok(ServiceDraft {
            name: self.name,
            category: Category::from_code(&self.category).ok_or(HttpError::InvalidCategory)?,
            description: self.description,
            monthly_price_cents: self.monthly_price_cents,
        })
    }
}

#[derive(Serialize)]
pub(crate) struct ServiceOutput {
    id: u64,
    name: String,
    category: &'static str,
    description: String,
    monthly_price_cents: u64,
}

impl From<DatacenterService> for ServiceOutput {
    fn from(service: DatacenterService) -> Self {
        Self {
            id: service.id,
            name: service.data.name,
            category: service.data.category.code(),
            description: service.data.description,
            monthly_price_cents: service.data.monthly_price_cents,
        }
    }
}

async fn list(
    State(catalog): State<CatalogService>,
    Extension(claims): Extension<Claims>,
) -> Result<Json<Vec<ServiceOutput>>, HttpError> {
    claims.require_any_role(&["users", "admin"])?;
    Ok(Json(
        catalog
            .list(&claims.actor())?
            .into_iter()
            .map(ServiceOutput::from)
            .collect(),
    ))
}

async fn find(
    State(catalog): State<CatalogService>,
    Extension(claims): Extension<Claims>,
    Path(id): Path<u64>,
) -> Result<Json<ServiceOutput>, HttpError> {
    claims.require_any_role(&["users", "admin"])?;
    Ok(Json(catalog.find(&claims.actor(), id)?.into()))
}

async fn create(
    State(catalog): State<CatalogService>,
    Extension(claims): Extension<Claims>,
    body: Result<Json<ServiceInput>, JsonRejection>,
) -> Result<(StatusCode, Json<ServiceOutput>), HttpError> {
    claims.require_any_role(&["admin"])?;
    let input = body.map_err(|_| HttpError::InvalidBody)?.0;
    Ok((
        StatusCode::CREATED,
        Json(catalog.create(&claims.actor(), input.into_draft()?)?.into()),
    ))
}

async fn update(
    State(catalog): State<CatalogService>,
    Extension(claims): Extension<Claims>,
    Path(id): Path<u64>,
    body: Result<Json<ServiceInput>, JsonRejection>,
) -> Result<Json<ServiceOutput>, HttpError> {
    claims.require_any_role(&["admin"])?;
    let input = body.map_err(|_| HttpError::InvalidBody)?.0;
    Ok(Json(
        catalog
            .update(&claims.actor(), id, input.into_draft()?)?
            .into(),
    ))
}

async fn delete(
    State(catalog): State<CatalogService>,
    Extension(claims): Extension<Claims>,
    Path(id): Path<u64>,
) -> Result<StatusCode, HttpError> {
    claims.require_any_role(&["admin"])?;
    catalog.delete(&claims.actor(), id)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn seed(
    State(catalog): State<CatalogService>,
    Extension(claims): Extension<Claims>,
) -> Result<Json<serde_json::Value>, HttpError> {
    claims.require_any_role(&["catalog-seed"])?;
    let entries = demo_catalog::load().map_err(|error| {
        tracing::error!(%error, "Catalogue de démonstration illisible");
        HttpError::InvalidSeed
    })?;
    let count = catalog.replace_all(&claims.actor(), entries)?;
    Ok(Json(serde_json::json!({ "count": count })))
}

enum HttpError {
    Security(ApiError),
    Catalog(CatalogError),
    InvalidCategory,
    InvalidBody,
    InvalidSeed,
}

impl From<ApiError> for HttpError {
    fn from(error: ApiError) -> Self {
        Self::Security(error)
    }
}

impl From<CatalogError> for HttpError {
    fn from(error: CatalogError) -> Self {
        Self::Catalog(error)
    }
}

impl IntoResponse for HttpError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            Self::Security(error) => return error.into_response(),
            Self::Catalog(CatalogError::PermissionDenied) => {
                return ApiError::Forbidden.into_response();
            }
            Self::Catalog(CatalogError::ServiceNotFound) => {
                (StatusCode::NOT_FOUND, "Service introuvable")
            }
            Self::InvalidCategory => (StatusCode::BAD_REQUEST, "Catégorie inconnue"),
            Self::InvalidBody => (
                StatusCode::BAD_REQUEST,
                "Corps JSON incompatible avec un service",
            ),
            Self::InvalidSeed => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Catalogue de démonstration indisponible",
            ),
        };
        (status, Json(serde_json::json!({ "message": message }))).into_response()
    }
}

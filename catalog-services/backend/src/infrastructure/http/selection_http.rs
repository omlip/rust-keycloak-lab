use super::catalog_http::ServiceOutput;
use crate::{
    application::{
        access::Actor,
        selection_service::{SelectionDetails, SelectionService},
    },
    domain::selection::SelectionError,
    infrastructure::auth::{ApiError, Claims},
};
use axum::{
    Extension, Json, Router,
    extract::{Path, State, rejection::JsonRejection},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, put},
};
use serde::{Deserialize, Serialize};

pub fn routes(selections: SelectionService) -> Router {
    // Resource IDs are process-local. Let monitoring clients detect a backend
    // restart instead of attaching an old threshold to a newly reused ID.
    let instance =
        axum::http::HeaderValue::from_str(&format!("{:032x}", rand::random::<u128>())).unwrap();
    Router::new()
        .route("/api/selections", get(list).post(create))
        .route(
            "/api/selections/{id}",
            get(find).patch(rename).delete(delete),
        )
        .route(
            "/api/selections/{id}/services/{service_id}",
            put(add_service).delete(remove_service),
        )
        .with_state(selections)
        .layer(axum::middleware::map_response(
            move |mut response: Response| {
                response
                    .headers_mut()
                    .insert("x-catalog-instance", instance.clone());
                async { response }
            },
        ))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NameInput {
    name: String,
}

#[derive(Serialize)]
struct SelectionOutput {
    id: u64,
    name: String,
    services: Vec<ServiceOutput>,
    monthly_total_cents: u64,
}

impl From<SelectionDetails> for SelectionOutput {
    fn from(details: SelectionDetails) -> Self {
        Self {
            id: details.id,
            name: details.name,
            services: details
                .services
                .into_iter()
                .map(ServiceOutput::from)
                .collect(),
            monthly_total_cents: details.monthly_total_cents,
        }
    }
}

fn actor(claims: Claims) -> Result<Actor, HttpError> {
    claims.require_any_role(&["users", "admin"])?;
    Ok(claims.actor())
}

async fn list(
    State(service): State<SelectionService>,
    Extension(claims): Extension<Claims>,
) -> Result<Json<Vec<SelectionOutput>>, HttpError> {
    Ok(Json(
        service
            .list(&actor(claims)?)?
            .into_iter()
            .map(SelectionOutput::from)
            .collect(),
    ))
}

async fn create(
    State(service): State<SelectionService>,
    Extension(claims): Extension<Claims>,
    body: Result<Json<NameInput>, JsonRejection>,
) -> Result<(StatusCode, Json<SelectionOutput>), HttpError> {
    let actor = actor(claims)?;
    let input = body.map_err(|_| HttpError::InvalidBody)?.0;
    Ok((
        StatusCode::CREATED,
        Json(service.create(&actor, &input.name)?.into()),
    ))
}

async fn find(
    State(service): State<SelectionService>,
    Extension(claims): Extension<Claims>,
    Path(id): Path<u64>,
) -> Result<Json<SelectionOutput>, HttpError> {
    Ok(Json(service.find(&actor(claims)?, id)?.into()))
}

async fn rename(
    State(service): State<SelectionService>,
    Extension(claims): Extension<Claims>,
    Path(id): Path<u64>,
    body: Result<Json<NameInput>, JsonRejection>,
) -> Result<Json<SelectionOutput>, HttpError> {
    let actor = actor(claims)?;
    let input = body.map_err(|_| HttpError::InvalidBody)?.0;
    Ok(Json(service.rename(&actor, id, &input.name)?.into()))
}

async fn delete(
    State(service): State<SelectionService>,
    Extension(claims): Extension<Claims>,
    Path(id): Path<u64>,
) -> Result<StatusCode, HttpError> {
    service.delete(&actor(claims)?, id)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn add_service(
    State(service): State<SelectionService>,
    Extension(claims): Extension<Claims>,
    Path((id, service_id)): Path<(u64, u64)>,
) -> Result<Json<SelectionOutput>, HttpError> {
    Ok(Json(
        service.add_service(&actor(claims)?, id, service_id)?.into(),
    ))
}

async fn remove_service(
    State(service): State<SelectionService>,
    Extension(claims): Extension<Claims>,
    Path((id, service_id)): Path<(u64, u64)>,
) -> Result<Json<SelectionOutput>, HttpError> {
    Ok(Json(
        service
            .remove_service(&actor(claims)?, id, service_id)?
            .into(),
    ))
}

enum HttpError {
    Security(ApiError),
    Selection(SelectionError),
    InvalidBody,
}

impl From<ApiError> for HttpError {
    fn from(error: ApiError) -> Self {
        Self::Security(error)
    }
}
impl From<SelectionError> for HttpError {
    fn from(error: SelectionError) -> Self {
        Self::Selection(error)
    }
}

impl IntoResponse for HttpError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            Self::Security(error) => return error.into_response(),
            Self::Selection(SelectionError::PermissionDenied) => {
                return ApiError::Forbidden.into_response();
            }
            Self::Selection(SelectionError::NotFound | SelectionError::AccessDenied) => {
                (StatusCode::NOT_FOUND, "Sélection introuvable")
            }
            Self::Selection(SelectionError::ServiceNotFound) => {
                (StatusCode::NOT_FOUND, "Service introuvable")
            }
            Self::Selection(SelectionError::InvalidName) => {
                (StatusCode::BAD_REQUEST, "Le nom ne doit pas être vide")
            }
            Self::Selection(SelectionError::TotalOverflow) => (
                StatusCode::UNPROCESSABLE_ENTITY,
                "Le total dépasse le montant maximum supporté",
            ),
            Self::Selection(SelectionError::Conflict) => (
                StatusCode::CONFLICT,
                "Modification concurrente, veuillez réessayer",
            ),
            Self::InvalidBody => (
                StatusCode::BAD_REQUEST,
                "Corps JSON incompatible avec une sélection",
            ),
        };
        (status, Json(serde_json::json!({ "message": message }))).into_response()
    }
}

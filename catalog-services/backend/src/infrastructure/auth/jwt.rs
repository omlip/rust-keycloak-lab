use super::jwks::{JwksClient, JwksError};
use crate::application::access::{Actor, Permission};
use axum::{
    Json,
    extract::{Request, State},
    http::{StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
};
use headers::{Authorization, HeaderMapExt, authorization::Bearer};
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode, decode_header};
use serde::Deserialize;
use std::{error::Error, sync::Arc};

pub struct AuthConfig {
    pub issuer: String,
    pub audience: String,
    pub jwks_url: String,
}

#[derive(Clone)]
pub struct Authenticator(Arc<Inner>);

struct Inner {
    validation: Validation,
    jwks: JwksClient,
}

#[derive(Clone, Deserialize)]
pub struct Claims {
    pub sub: String,
    pub exp: u64,
    pub iss: String,
    pub aud: serde_json::Value,
    pub typ: String,
    #[serde(default)]
    pub realm_access: RealmAccess,
    #[serde(default)]
    pub scope: String,
    #[serde(default)]
    pub azp: String,
}

#[derive(Clone, Default, Deserialize)]
pub struct RealmAccess {
    #[serde(default)]
    pub roles: Vec<String>,
}

impl Claims {
    pub fn actor(&self) -> Actor {
        use Permission::*;
        // Intersect user roles, granted OAuth scopes and the registered client's
        // maximum capabilities. Missing/unknown clients or scopes fail closed.
        let allowed: &[Permission] = match self.azp.as_str() {
            "olaninc-spa" => &[CatalogRead, CatalogWrite, SelectionsRead, SelectionsWrite],
            "cost-monitor" => &[SelectionsRead],
            "catalog-loader" => &[CatalogSeed],
            _ => &[],
        };
        let has = |role| self.realm_access.roles.iter().any(|r| r == role);
        let permissions: Vec<_> = allowed
            .iter()
            .copied()
            .filter(|p| {
                let role_allowed = match p {
                    CatalogWrite => has("admin"),
                    CatalogSeed => has("catalog-seed"),
                    _ => has("users") || has("admin"),
                };
                role_allowed
                    && self
                        .scope
                        .split_whitespace()
                        .any(|s| s == permission_scope(*p))
            })
            .collect();
        Actor::new(self.sub.clone(), &permissions)
    }
    pub fn require_any_role(&self, allowed: &[&str]) -> Result<(), ApiError> {
        if allowed
            .iter()
            .any(|role| self.realm_access.roles.iter().any(|r| r == role))
        {
            Ok(())
        } else {
            Err(ApiError::Forbidden)
        }
    }
}

// OAuth vocabulary belongs to the authentication adapter, not application permissions.
fn permission_scope(permission: Permission) -> &'static str {
    match permission {
        Permission::CatalogRead => "catalog:read",
        Permission::CatalogWrite => "catalog:write",
        Permission::CatalogSeed => "catalog:seed",
        Permission::SelectionsRead => "selections:read",
        Permission::SelectionsWrite => "selections:write",
    }
}

impl Authenticator {
    pub fn new(config: AuthConfig) -> Result<Self, Box<dyn Error>> {
        if config.issuer.is_empty() || config.audience.is_empty() {
            return Err("OIDC_ISSUER et OIDC_AUDIENCE doivent être renseignés".into());
        }
        reqwest::Url::parse(&config.issuer)?;
        let jwks = JwksClient::new(config.jwks_url)?;
        let mut validation = Validation::new(Algorithm::RS256);
        validation.set_issuer(&[config.issuer]);
        validation.set_audience(&[config.audience]);
        validation.set_required_spec_claims(&["exp", "iss", "aud", "sub"]);
        validation.leeway = 0;
        validation.validate_nbf = true;
        Ok(Self(Arc::new(Inner { validation, jwks })))
    }

    pub async fn warm_up(&self) -> Result<(), ApiError> {
        self.0.jwks.warm_up().await.map_err(ApiError::from)
    }

    async fn key(&self, kid: &str) -> Result<DecodingKey, ApiError> {
        let jwk = self.0.jwks.key(kid).await?;
        // Restrict both the JWT algorithm and the key's advertised use/algorithm.
        if jwk
            .common
            .public_key_use
            .as_ref()
            .is_some_and(|usage| *usage != jsonwebtoken::jwk::PublicKeyUse::Signature)
            || jwk
                .common
                .key_algorithm
                .is_some_and(|alg| alg != jsonwebtoken::jwk::KeyAlgorithm::RS256)
        {
            return Err(ApiError::Unauthorized);
        }
        DecodingKey::from_jwk(&jwk).map_err(|_| ApiError::Unauthorized)
    }

    pub async fn verify(&self, token: &str) -> Result<Claims, ApiError> {
        let header = decode_header(token).map_err(|_| ApiError::Unauthorized)?;
        if header.alg != Algorithm::RS256 {
            return Err(ApiError::Unauthorized);
        }
        let kid = header
            .kid
            .filter(|kid| !kid.is_empty())
            .ok_or(ApiError::Unauthorized)?;
        let key = self.key(&kid).await?;
        let claims = decode::<Claims>(token, &key, &self.0.validation)
            .map_err(|_| ApiError::Unauthorized)?
            .claims;
        // Keycloak access tokens carry typ=Bearer; ID and refresh tokens are not API credentials.
        if claims.typ != "Bearer" || claims.sub.is_empty() {
            return Err(ApiError::Unauthorized);
        }
        Ok(claims)
    }
}

pub async fn authenticate(
    State(auth): State<Authenticator>,
    mut request: Request,
    next: Next,
) -> Result<Response, ApiError> {
    if request
        .headers()
        .get_all(header::AUTHORIZATION)
        .iter()
        .count()
        != 1
    {
        return Err(ApiError::Unauthorized);
    }
    let authorization = request
        .headers()
        .typed_get::<Authorization<Bearer>>()
        .ok_or(ApiError::Unauthorized)?;
    let claims = auth.verify(authorization.token()).await?;
    request.extensions_mut().insert(claims);
    Ok(next.run(request).await)
}

#[derive(Debug)]
pub enum ApiError {
    Unauthorized,
    Forbidden,
    Unavailable,
}

impl From<JwksError> for ApiError {
    fn from(error: JwksError) -> Self {
        match error {
            JwksError::KeyNotFound => Self::Unauthorized,
            JwksError::Unavailable => Self::Unavailable,
        }
    }
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl Error for ApiError {}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            Self::Unauthorized => (StatusCode::UNAUTHORIZED, "Jeton absent ou invalide"),
            Self::Forbidden => (StatusCode::FORBIDDEN, "Accès refusé"),
            Self::Unavailable => (
                StatusCode::SERVICE_UNAVAILABLE,
                "Vérification des jetons indisponible",
            ),
        };
        let mut response =
            (status, Json(serde_json::json!({ "message": message }))).into_response();
        response
            .headers_mut()
            .insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
        if status == StatusCode::UNAUTHORIZED {
            response
                .headers_mut()
                .insert(header::WWW_AUTHENTICATE, "Bearer".parse().unwrap());
        }
        response
    }
}

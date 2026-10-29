use crate::{
    alerts,
    auth::Authenticator,
    catalog,
    integrations::{ConnectionError, Integration, Integrations},
    mail::{CostAlert, Mailer},
    oidc::{Oidc, OidcError},
    sessions::{self, CatalogBinding, LIMIT, LOGIN_TTL, Pending, Session, Sessions},
};
use axum::{
    Json, Router,
    extract::{Form, Path, Query, Request, State},
    http::{HeaderMap, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Redirect, Response},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

pub struct Config {
    pub origin: String,
    pub issuer: String,
    pub api: String,
    pub allow_unverified_email: bool,
}
struct App {
    auth: Authenticator,
    catalog_oidc: Oidc,
    config: Config,
    sessions: Sessions,
    integrations: Integrations,
    http: reqwest::Client,
    mailer: Arc<dyn Mailer>,
}
pub fn app(
    auth: Authenticator,
    catalog_oidc: Oidc,
    config: Config,
    mailer: Arc<dyn Mailer>,
) -> (Router, tokio::task::JoinHandle<()>) {
    let state = Arc::new(App {
        auth,
        catalog_oidc,
        config,
        sessions: Sessions::default(),
        integrations: Integrations::default(),
        http: reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(15))
            .build()
            .expect("HTTP client"),
        mailer,
    });
    let worker = start_monitoring(state.clone());
    (router(state), worker)
}
fn router(state: Arc<App>) -> Router {
    Router::new()
        .route("/api/dashboard", get(home))
        .route("/api/catalog/connect", post(connect_catalog))
        .route("/oidc/catalog/callback", get(catalog_callback))
        .route("/api/catalog/complete", post(complete_catalog))
        .route("/api/catalog/refresh", post(refresh))
        .route("/api/catalog/disconnect", post(disconnect_catalog))
        .route("/api/logout", post(logout))
        .route("/api/alerts/{id}", post(save_alert))
        .route("/api/alerts/{id}/delete", post(delete_alert))
        .route("/health", get(|| async { "ok" }))
        .with_state(state.clone())
        .layer(middleware::from_fn_with_state(state, security_headers))
}
async fn security_headers(State(app): State<Arc<App>>, request: Request, next: Next) -> Response {
    // Only the server OIDC callback is browser navigation; APIs always keep JSON statuses.
    let navigation = request.uri().path() == "/oidc/catalog/callback"
        && request
            .headers()
            .get(header::ACCEPT)
            .and_then(|h| h.to_str().ok())
            .is_some_and(|h| h.contains("text/html"));
    let mut response = next.run(request).await;
    if navigation && (response.status().is_client_error() || response.status().is_server_error()) {
        let bytes = axum::body::to_bytes(response.into_body(), 16384)
            .await
            .unwrap_or_default();
        let error: serde_json::Value = serde_json::from_slice(&bytes).unwrap_or_default();
        let message = error["message"]
            .as_str()
            .unwrap_or("La connexion catalogue a échoué.");
        let url = reqwest::Url::parse_with_params(&app.config.origin, &[("error", message)])
            .expect("application origin");
        response = Redirect::to(url.as_str()).into_response();
    }
    for (name, value) in [
        ("cache-control", "no-store"),
        ("referrer-policy", "no-referrer"),
        ("x-content-type-options", "nosniff"),
        ("x-frame-options", "DENY"),
        (
            "content-security-policy",
            "default-src 'none'; frame-ancestors 'none'; base-uri 'none'",
        ),
    ] {
        response.headers_mut().insert(
            axum::http::HeaderName::from_static(name),
            value.parse().unwrap(),
        );
    }
    response
}
fn cookie<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers
        .get(header::COOKIE)?
        .to_str()
        .ok()?
        .split(';')
        .find_map(|entry| {
            let (key, value) = entry.trim().split_once('=')?;
            (key == name).then_some(value)
        })
}
fn set_cookie(response: &mut Response, config: &Config, name: &str, value: &str, age: u64) {
    let secure = if config.origin.starts_with("https://") {
        "; Secure"
    } else {
        ""
    };
    response.headers_mut().append(
        header::SET_COOKIE,
        format!("{name}={value}; Path=/; HttpOnly; SameSite=Lax; Max-Age={age}{secure}")
            .parse()
            .unwrap(),
    );
}
#[derive(Debug)]
struct Error(StatusCode, &'static str);
impl IntoResponse for Error {
    fn into_response(self) -> Response {
        let mut response = (self.0, Json(serde_json::json!({"message":self.1}))).into_response();
        if self.0 == StatusCode::UNAUTHORIZED {
            response
                .headers_mut()
                .insert(header::WWW_AUTHENTICATE, "Bearer".parse().unwrap());
        }
        response
    }
}
fn connection_error(error: ConnectionError) -> Error {
    match error {
        ConnectionError::AccountMismatch => Error(
            StatusCode::FORBIDDEN,
            "Le compte catalogue doit être le même que celui connecté à Cost Monitor. Aucune connexion catalogue n’a été enregistrée.",
        ),
        ConnectionError::SessionUnavailable => Error(
            StatusCode::UNAUTHORIZED,
            "La session Cost Monitor est fermée ou expirée. Reconnecte-toi.",
        ),
        ConnectionError::StaleAttempt => Error(
            StatusCode::CONFLICT,
            "Cette tentative de connexion catalogue a été annulée ou remplacée.",
        ),
        ConnectionError::Capacity => Error(
            StatusCode::SERVICE_UNAVAILABLE,
            "Nombre maximum de connexions atteint pour cette démonstration en mémoire.",
        ),
    }
}
async fn authorized_session(
    app: &App,
    headers: &HeaderMap,
    mutation: bool,
) -> Result<(String, Arc<tokio::sync::Mutex<Session>>), Error> {
    // Bearer validation is mandatory, even when a pending-browser cookie exists.
    let principal = app.auth.verify(headers).map_err(|_| {
        Error(
            StatusCode::UNAUTHORIZED,
            "Jeton Cost Monitor absent, expiré ou invalide.",
        )
    })?;
    if mutation
        && headers.get(header::ORIGIN).and_then(|h| h.to_str().ok())
            != Some(app.config.origin.as_str())
    {
        return Err(Error(
            StatusCode::FORBIDDEN,
            "Origine de la requête refusée.",
        ));
    }
    let context = app
        .sessions
        .bind(&principal)
        .await
        .map_err(connection_error)?;
    Ok((principal.sid, context))
}
async fn connect_catalog(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
) -> Result<Response, Error> {
    let (sid, session) = authorized_session(&app, &headers, true).await?;
    let value = session.lock().await;
    value
        .ensure_account(&value.identity)
        .map_err(connection_error)?;
    let integration = app
        .integrations
        .get_or_create(&value.identity)
        .await
        .map_err(connection_error)?;
    let epoch = integration.lock().await.begin_catalog();
    app.sessions.cancel_pending(&sid).await;
    let login = app.catalog_oidc.login();
    let mut response = Json(serde_json::json!({"url":login.url})).into_response();
    let mut pending = app.sessions.pending.lock().await;
    pending.retain(|_, p| p.created.elapsed() < LOGIN_TTL);
    if let Some(old) = cookie(&headers, "cm_catalog_login") {
        pending.remove(old);
    }
    if pending.len() >= LIMIT {
        return Err(Error(
            StatusCode::SERVICE_UNAVAILABLE,
            "Trop de connexions catalogue en cours.",
        ));
    }
    let id = sessions::random();
    pending.insert(
        id.clone(),
        Pending {
            login,
            created: Instant::now(),
            code: None,
            binding: CatalogBinding {
                session_id: sid,
                session: session.clone(),
                epoch,
            },
        },
    );
    set_cookie(
        &mut response,
        &app.config,
        "cm_catalog_login",
        &id,
        LOGIN_TTL.as_secs(),
    );
    Ok(response)
}
#[derive(Deserialize)]
struct Callback {
    state: Option<String>,
    code: Option<String>,
    error: Option<String>,
    iss: Option<String>,
}
async fn catalog_callback(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Query(query): Query<Callback>,
) -> Result<Response, Error> {
    let invalid = || {
        Error(
            StatusCode::BAD_REQUEST,
            "Autorisation refusée, expirée ou invalide. Recommence la connexion catalogue.",
        )
    };
    let id = cookie(&headers, "cm_catalog_login").ok_or_else(invalid)?;
    let mut pending = app.sessions.pending.lock().await;
    let attempt = pending.get_mut(id).ok_or_else(invalid)?;
    if attempt.created.elapsed() >= LOGIN_TTL
        || attempt.code.is_some()
        || !sessions::equal_secret(
            query.state.as_deref().unwrap_or(""),
            attempt.login.state.secret(),
        )
        || query.error.is_some()
        || query
            .iss
            .as_ref()
            .is_some_and(|iss| iss != &app.config.issuer)
        || query.code.as_ref().is_none_or(|code| code.is_empty())
    {
        pending.remove(id);
        return Err(invalid());
    }
    // No exchange/attachment here: this GET does not carry the SPA's Bearer token.
    // Drop the pending map lock before waiting for a session used by an API call.
    attempt.code = query.code;
    Ok(Redirect::to("/?catalog_return=1").into_response())
}
async fn complete_catalog(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
) -> Result<Response, Error> {
    let (sid, current) = authorized_session(&app, &headers, true).await?;
    let Some(id) = cookie(&headers, "cm_catalog_login") else {
        return Ok(StatusCode::NO_CONTENT.into_response());
    };
    let mut response = StatusCode::NO_CONTENT.into_response();
    let pending = {
        let mut pending = app.sessions.pending.lock().await;
        if pending
            .get(id)
            .is_some_and(|p| p.code.is_none() && p.created.elapsed() < LOGIN_TTL)
        {
            return Ok(response);
        }
        pending.remove(id)
    };
    set_cookie(&mut response, &app.config, "cm_catalog_login", "", 0);
    let Some(pending) = pending else {
        return Ok(response);
    };
    if pending.created.elapsed() >= LOGIN_TTL {
        return Err(Error(
            StatusCode::BAD_REQUEST,
            "La connexion catalogue a expiré.",
        ));
    }
    if pending.binding.session_id != sid || !Arc::ptr_eq(&pending.binding.session, &current) {
        return Err(Error(
            StatusCode::FORBIDDEN,
            "La session ne correspond plus à cette connexion catalogue.",
        ));
    }
    let identity = {
        let value = current.lock().await;
        value
            .ensure_account(&value.identity)
            .map_err(connection_error)?;
        value.identity.clone()
    };
    let integration = app
        .integrations
        .get(&identity)
        .await
        .ok_or(connection_error(ConnectionError::StaleAttempt))?;
    integration
        .lock()
        .await
        .validate_attempt(&identity, pending.binding.epoch)
        .map_err(connection_error)?;
    let (catalog_identity, tokens) = app
        .catalog_oidc
        .exchange(
            pending
                .code
                .ok_or(Error(StatusCode::BAD_REQUEST, "Code catalogue absent."))?,
            pending.login.verifier,
            &pending.login.nonce,
        )
        .await
        .map_err(|_| {
            Error(
                StatusCode::BAD_REQUEST,
                "Autorisation catalogue refusée ou invalide.",
            )
        })?;
    // Logout/disconnect can run during the network exchange; never resurrect an attempt.
    let value = current.lock().await;
    value
        .ensure_account(&catalog_identity)
        .map_err(connection_error)?;
    integration
        .lock()
        .await
        .attach_catalog(
            &catalog_identity,
            tokens,
            pending.login.nonce,
            pending.binding.epoch,
        )
        .map_err(connection_error)?;
    Ok(response)
}
// Renewal uses only the account integration, never the SPA's access/refresh token.
async fn renew(app: &App, integration: &mut Integration, force: bool) -> Result<(), &'static str> {
    let subject = integration.subject().to_owned();
    let Some(connection) = integration.catalog.as_mut() else {
        return Ok(());
    };
    if !force && !connection.needs_refresh() {
        return Ok(());
    }
    let Some(tokens) = connection.tokens.as_ref() else {
        return Ok(());
    };
    match app
        .catalog_oidc
        .refresh(&tokens.offline, &connection.nonce, &subject)
        .await
    {
        Ok(tokens) => {
            connection.replace(tokens);
            Ok(())
        }
        Err(OidcError::Revoked | OidcError::InvalidAuthorization) => {
            connection.tokens = None;
            Err("L’accès au catalogue a expiré ou a été révoqué. Reconnecte ton catalogue.")
        }
        Err(OidcError::Unavailable) => Err(
            "La connexion au catalogue est temporairement indisponible. Réessaie dans un instant.",
        ),
    }
}
#[derive(Serialize)]
struct Dashboard {
    name: String,
    connected: bool,
    has_catalog: bool,
    recipient: Option<String>,
    message: Option<&'static str>,
    selections: Vec<SelectionView>,
}
#[derive(Serialize)]
struct SelectionView {
    id: String,
    name: String,
    monthly_total_cents: String,
    threshold_cents: Option<String>,
    services: Vec<ServiceView>,
}
#[derive(Serialize)]
struct ServiceView {
    name: String,
    monthly_price_cents: String,
}
async fn load_dashboard(app: &App, session: &mut Session, force: bool) -> Result<Dashboard, Error> {
    session
        .ensure_account(&session.identity)
        .map_err(connection_error)?;
    let mut result = Dashboard {
        name: session.identity.name.clone(),
        connected: false,
        has_catalog: false,
        recipient: None,
        message: None,
        selections: vec![],
    };
    let Some(integration) = app.integrations.get(&session.identity).await else {
        return Ok(result);
    };
    let mut integration = integration.lock().await;
    let selections = match fetch_catalog(app, &mut integration, force).await {
        Ok(Some(snapshot)) => snapshot.selections,
        Ok(None) => vec![],
        Err(message) => {
            result.message = Some(message);
            vec![]
        }
    };
    result.connected = integration
        .catalog
        .as_ref()
        .is_some_and(|c| c.tokens.is_some());
    result.has_catalog = integration.catalog.is_some();
    result.recipient =
        valid_recipient(&integration, app.config.allow_unverified_email).map(str::to_owned);
    result.selections = selections
        .into_iter()
        .map(|s| SelectionView {
            id: s.id.to_string(),
            name: s.name,
            monthly_total_cents: s.monthly_total_cents.to_string(),
            threshold_cents: integration
                .budgets
                .rules
                .get(&s.id)
                .map(|r| r.threshold_cents.to_string()),
            services: s
                .services
                .into_iter()
                .map(|s| ServiceView {
                    name: s.name,
                    monthly_price_cents: s.monthly_price_cents.to_string(),
                })
                .collect(),
        })
        .collect();
    Ok(result)
}
async fn fetch_catalog(
    app: &App,
    integration: &mut Integration,
    force: bool,
) -> Result<Option<catalog::Snapshot>, &'static str> {
    renew(app, integration, force).await?;
    let Some(tokens) = integration.catalog.as_ref().and_then(|c| c.tokens.as_ref()) else {
        return Ok(None);
    };
    let snapshot = catalog::read(&app.http, &app.config.api, tokens.access.secret()).await?;
    if let Some(instance) = &snapshot.instance {
        integration.budgets.synchronize(
            instance,
            &snapshot.selections.iter().map(|s| s.id).collect::<Vec<_>>(),
        );
    }
    Ok(Some(snapshot))
}
fn valid_recipient(integration: &Integration, allow_unverified: bool) -> Option<&str> {
    let recipient = integration
        .catalog
        .as_ref()?
        .tokens
        .as_ref()?
        .recipient
        .as_ref()?;
    if (!recipient.verified && !allow_unverified)
        || recipient
            .address
            .parse::<lettre::message::Mailbox>()
            .is_err()
    {
        return None;
    }
    Some(&recipient.address)
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AlertInput {
    threshold: String,
}
async fn save_alert(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Path(id): Path<u64>,
    Form(input): Form<AlertInput>,
) -> Result<StatusCode, Error> {
    let (_, session) = authorized_session(&app, &headers, true).await?;
    let value = session.lock().await;
    value
        .ensure_account(&value.identity)
        .map_err(connection_error)?;
    let integration = app.integrations.get(&value.identity).await.ok_or(Error(
        StatusCode::CONFLICT,
        "Connecte d’abord ton catalogue.",
    ))?;
    let mut integration = integration.lock().await;
    let snapshot = fetch_catalog(&app, &mut integration, false)
        .await
        .map_err(|m| Error(StatusCode::BAD_GATEWAY, m))?
        .ok_or(Error(StatusCode::CONFLICT, "Reconnecte ton catalogue."))?;
    if snapshot.instance.is_none() {
        return Err(Error(
            StatusCode::BAD_GATEWAY,
            "Le catalogue doit être mis à jour pour activer la surveillance.",
        ));
    }
    if !snapshot.selections.iter().any(|s| s.id == id) {
        return Err(Error(StatusCode::NOT_FOUND, "Sélection introuvable."));
    }
    if valid_recipient(&integration, app.config.allow_unverified_email).is_none() {
        return Err(Error(
            StatusCode::BAD_REQUEST,
            "Une adresse e-mail autorisée est nécessaire. Vérifie ton profil Keycloak puis reconnecte le catalogue.",
        ));
    }
    let cents =
        alerts::parse_euros(&input.threshold).map_err(|m| Error(StatusCode::BAD_REQUEST, m))?;
    integration
        .budgets
        .configure(id, cents)
        .map_err(|m| Error(StatusCode::BAD_REQUEST, m))?;
    Ok(StatusCode::NO_CONTENT)
}
async fn delete_alert(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Path(id): Path<u64>,
) -> Result<StatusCode, Error> {
    let (_, session) = authorized_session(&app, &headers, true).await?;
    let value = session.lock().await;
    value
        .ensure_account(&value.identity)
        .map_err(connection_error)?;
    let integration = app
        .integrations
        .get(&value.identity)
        .await
        .ok_or(Error(StatusCode::NOT_FOUND, "Alerte introuvable."))?;
    if integration.lock().await.budgets.rules.remove(&id).is_none() {
        return Err(Error(StatusCode::NOT_FOUND, "Alerte introuvable."));
    }
    Ok(StatusCode::NO_CONTENT)
}
async fn home(State(app): State<Arc<App>>, headers: HeaderMap) -> Result<Json<Dashboard>, Error> {
    let (_, session) = authorized_session(&app, &headers, false).await?;
    Ok(Json(
        load_dashboard(&app, &mut *session.lock().await, false).await?,
    ))
}
async fn refresh(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
) -> Result<Json<Dashboard>, Error> {
    let (_, session) = authorized_session(&app, &headers, true).await?;
    Ok(Json(
        load_dashboard(&app, &mut *session.lock().await, true).await?,
    ))
}
async fn disconnect_catalog(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
) -> Result<Response, Error> {
    let (sid, session) = authorized_session(&app, &headers, true).await?;
    let value = session.lock().await;
    value
        .ensure_account(&value.identity)
        .map_err(connection_error)?;
    if let Some(integration) = app.integrations.get(&value.identity).await {
        integration.lock().await.disconnect_catalog();
    }
    app.sessions.cancel_pending(&sid).await;
    let mut response = StatusCode::NO_CONTENT.into_response();
    set_cookie(&mut response, &app.config, "cm_catalog_login", "", 0);
    Ok(response)
}
async fn logout(State(app): State<Arc<App>>, headers: HeaderMap) -> Result<Response, Error> {
    let (sid, session) = authorized_session(&app, &headers, true).await?;
    session.lock().await.close();
    app.sessions.cancel_pending(&sid).await;
    // The SPA performs the Keycloak logout using its own in-memory ID token.
    // This endpoint only cancels delegation attempts, never deletes offline grants.
    let mut response = StatusCode::NO_CONTENT.into_response();
    for name in ["cm_catalog_login", "cm_session", "cm_app_login"] {
        set_cookie(&mut response, &app.config, name, "", 0);
    }
    Ok(response)
}
fn start_monitoring(app: Arc<App>) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(60));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        interval.tick().await;
        loop {
            interval.tick().await;
            check_once(app.clone()).await;
        }
    })
}
async fn check_once(app: Arc<App>) {
    let semaphore = Arc::new(tokio::sync::Semaphore::new(8));
    let mut tasks = tokio::task::JoinSet::new();
    for slot in app.integrations.entries().await {
        let app = app.clone();
        let semaphore = semaphore.clone();
        tasks.spawn(async move {
            let Ok(_permit) = semaphore.acquire_owned().await else {
                return;
            };
            let mut integration = slot.lock().await;
            if integration.budgets.rules.is_empty()
                || integration
                    .catalog
                    .as_ref()
                    .and_then(|c| c.tokens.as_ref())
                    .is_none()
            {
                return;
            }
            let snapshot = match fetch_catalog(&app, &mut integration, false).await {
                Ok(Some(snapshot)) if snapshot.instance.is_some() => snapshot,
                _ => return,
            };
            let recipient =
                valid_recipient(&integration, app.config.allow_unverified_email).map(str::to_owned);
            for selection in &snapshot.selections {
                let Some(rule) = integration.budgets.rules.get_mut(&selection.id) else {
                    continue;
                };
                let Some(notification) = rule.notification(selection.monthly_total_cents) else {
                    continue;
                };
                let Some(recipient) = recipient.as_deref() else {
                    continue;
                };
                if app
                    .mailer
                    .send(CostAlert {
                        recipient,
                        selection,
                        threshold: rule.threshold_cents,
                        notification,
                    })
                    .await
                    .is_ok()
                {
                    rule.delivered(notification);
                } else {
                    eprintln!("Cost monitoring: email delivery failed; retry on next check.");
                }
            }
        });
    }
    while let Some(result) = tasks.join_next().await {
        if result.is_err() {
            eprintln!("Cost monitoring: an account check failed.");
        }
    }
}
#[cfg(test)]
#[path = "web_tests.rs"]
mod tests;

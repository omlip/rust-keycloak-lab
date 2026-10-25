use crate::{
    alerts, catalog,
    integrations::{ConnectionError, Integration, Integrations},
    mail::{CostAlert, Mailer},
    oidc::{Oidc, OidcError, Purpose},
    sessions::{self, CatalogBinding, LIMIT, LOGIN_TTL, Pending, SESSION_TTL, Session, Sessions},
    view,
};
use axum::{
    Router,
    extract::{Form, Path, Query, Request, State},
    http::{HeaderMap, StatusCode, header},
    middleware::{self, Next},
    response::{Html, IntoResponse, Redirect, Response},
    routing::{get, post},
};
use serde::Deserialize;
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
    login_oidc: Oidc,
    catalog_oidc: Oidc,
    config: Config,
    sessions: Sessions,
    integrations: Integrations,
    http: reqwest::Client,
    mailer: Arc<dyn Mailer>,
}

pub fn app(
    login_oidc: Oidc,
    catalog_oidc: Oidc,
    config: Config,
    mailer: Arc<dyn Mailer>,
) -> (Router, tokio::task::JoinHandle<()>) {
    let state = Arc::new(App {
        login_oidc,
        catalog_oidc,
        config,
        sessions: Sessions::default(),
        integrations: Integrations::default(),
        mailer,
        http: reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(15))
            .build()
            .expect("HTTP client"),
    });
    let worker = start_monitoring(state.clone());
    (router(state), worker)
}

fn router(state: Arc<App>) -> Router {
    Router::new()
        .route("/", get(home))
        .route("/login", get(login))
        .route("/oidc/login/callback", get(login_callback))
        .route("/catalog/connect", post(connect_catalog))
        .route("/oidc/catalog/callback", get(catalog_callback))
        .route("/catalog/refresh", post(refresh))
        .route("/catalog/disconnect", post(disconnect_catalog))
        .route("/logout", post(logout))
        .route("/alerts/{id}", post(save_alert))
        .route("/alerts/{id}/delete", post(delete_alert))
        .route(
            "/dialogs.js",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "application/javascript")],
                    include_str!("../resources/dialogs.js"),
                )
            }),
        )
        .route("/health", get(|| async { "ok" }))
        .route(
            "/logo.svg",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "image/svg+xml")],
                    include_str!("../resources/logo.svg"),
                )
            }),
        )
        .with_state(state.clone())
        .layer(middleware::from_fn_with_state(state, security_headers))
}

async fn security_headers(State(app): State<Arc<App>>, request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    for (name, value) in [
        ("cache-control", "no-store"),
        ("referrer-policy", "strict-origin"),
        ("x-content-type-options", "nosniff"),
        ("x-frame-options", "DENY"),
    ] {
        headers.insert(
            axum::http::HeaderName::from_static(name),
            value.parse().unwrap(),
        );
    }
    // Chromium applies form-action to redirects too. Catalogue connection is a
    // CSRF-protected POST followed by a redirect to the configured OIDC issuer.
    // Permit that exact origin, never arbitrary destinations or wildcard hosts.
    let issuer_origin = reqwest::Url::parse(&app.config.issuer)
        .ok()
        .map(|url| url.origin().ascii_serialization())
        .unwrap_or_default();
    let csp = format!(
        "default-src 'none'; script-src 'self'; style-src 'unsafe-inline'; img-src 'self'; form-action 'self' {issuer_origin}; frame-ancestors 'none'; base-uri 'none'"
    );
    headers.insert(
        "content-security-policy",
        csp.parse().expect("validated issuer origin"),
    );
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
    let value = format!("{name}={value}; Path=/; HttpOnly; SameSite=Lax; Max-Age={age}{secure}");
    response
        .headers_mut()
        .append(header::SET_COOKIE, value.parse().expect("generated cookie"));
}

#[derive(Debug)]
struct Error(StatusCode, &'static str);
impl IntoResponse for Error {
    fn into_response(self) -> Response {
        (self.0, Html(view::page(&format!("<section><p role=\"alert\">{}</p><a href=\"/\">Retour à Cost Monitor</a></section>", view::escape(self.1))))).into_response()
    }
}

async fn login(State(app): State<Arc<App>>, headers: HeaderMap) -> Result<Response, Error> {
    if let Some(id) = cookie(&headers, "cm_session")
        && app.sessions.get(id).await.is_some()
    {
        return Ok(Redirect::to("/").into_response());
    }
    start_authorization(&app, &headers, Purpose::AppLogin, None).await
}

fn login_cookie(purpose: Purpose) -> &'static str {
    match purpose {
        Purpose::AppLogin => "cm_app_login",
        Purpose::Catalog => "cm_catalog_login",
    }
}

async fn start_authorization(
    app: &App,
    headers: &HeaderMap,
    purpose: Purpose,
    binding: Option<CatalogBinding>,
) -> Result<Response, Error> {
    let login = match purpose {
        Purpose::AppLogin => &app.login_oidc,
        Purpose::Catalog => &app.catalog_oidc,
    }
    .login();
    let mut response = Redirect::to(&login.url).into_response();
    let mut pending = app.sessions.pending.lock().await;
    pending.retain(|_, p| p.created.elapsed() < LOGIN_TTL);
    if let Some(previous) = cookie(headers, login_cookie(purpose)) {
        pending.remove(previous);
    }
    if pending.len() >= LIMIT {
        return Err(Error(
            StatusCode::SERVICE_UNAVAILABLE,
            "Trop de connexions en cours, réessaie plus tard.",
        ));
    }
    let id = sessions::random();
    pending.insert(
        id.clone(),
        Pending {
            login,
            created: Instant::now(),
            purpose,
            binding,
        },
    );
    set_cookie(
        &mut response,
        &app.config,
        login_cookie(purpose),
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

async fn take_authorization(
    app: &App,
    headers: &HeaderMap,
    query: &Callback,
    purpose: Purpose,
) -> Result<Pending, Error> {
    let invalid = || {
        Error(
            StatusCode::BAD_REQUEST,
            "Autorisation refusée, expirée ou invalide. Recommence la connexion.",
        )
    };
    let id = cookie(headers, login_cookie(purpose)).ok_or_else(invalid)?;
    let pending = app
        .sessions
        .pending
        .lock()
        .await
        .remove(id)
        .ok_or_else(invalid)?;
    if pending.purpose != purpose
        || pending.created.elapsed() >= LOGIN_TTL
        || !sessions::equal_secret(
            query.state.as_deref().unwrap_or(""),
            pending.login.state.secret(),
        )
        || query.error.is_some()
        || query.iss.as_ref().is_some_and(|i| i != &app.config.issuer)
    {
        return Err(invalid());
    }
    if query.code.is_none() {
        return Err(invalid());
    }
    Ok(pending)
}

async fn login_callback(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Query(query): Query<Callback>,
) -> Result<Response, Error> {
    let pending = take_authorization(&app, &headers, &query, Purpose::AppLogin).await?;
    let identity = app
        .login_oidc
        .authenticate(
            query.code.unwrap(),
            pending.login.verifier,
            &pending.login.nonce,
        )
        .await
        .map_err(|_| {
            Error(
                StatusCode::BAD_REQUEST,
                "Connexion Cost Monitor invalide. Recommence la connexion.",
            )
        })?;
    if let Some(previous) = cookie(&headers, "cm_session")
        && app.sessions.get(previous).await.is_some()
    {
        return Err(Error(
            StatusCode::CONFLICT,
            "Une session Cost Monitor est déjà ouverte dans ce navigateur.",
        ));
    }
    let session = Session::new(identity);
    let id = app.sessions.insert(session).await.ok_or(Error(
        StatusCode::SERVICE_UNAVAILABLE,
        "Trop de sessions actives.",
    ))?;
    let mut response = Redirect::to("/").into_response();
    set_cookie(&mut response, &app.config, "cm_app_login", "", 0);
    set_cookie(
        &mut response,
        &app.config,
        "cm_session",
        &id,
        SESSION_TTL.as_secs(),
    );
    Ok(response)
}

async fn connect_catalog(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Form(action): Form<Action>,
) -> Result<Response, Error> {
    let (id, session) = authorized_session(&app, &headers, &action).await?;
    let session = session.lock().await;
    session
        .ensure_account(&session.identity)
        .map_err(connection_error)?;
    let integration = app
        .integrations
        .get_or_create(&session.identity)
        .await
        .map_err(connection_error)?;
    let mut integration = integration.lock().await;
    let epoch = integration.begin_catalog();
    app.sessions.cancel_pending(&id).await;
    start_authorization(
        &app,
        &headers,
        Purpose::Catalog,
        Some(CatalogBinding {
            session_id: id,
            epoch,
        }),
    )
    .await
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
            "Nombre maximum d’intégrations atteint pour cette démonstration en mémoire.",
        ),
    }
}

async fn catalog_callback(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Query(query): Query<Callback>,
) -> Result<Response, Error> {
    let pending = take_authorization(&app, &headers, &query, Purpose::Catalog).await?;
    let binding = pending.binding.ok_or(Error(
        StatusCode::BAD_REQUEST,
        "Connexion catalogue non liée à une session.",
    ))?;
    if !cookie(&headers, "cm_session")
        .is_some_and(|id| sessions::equal_secret(id, &binding.session_id))
    {
        return Err(Error(
            StatusCode::UNAUTHORIZED,
            "La session Cost Monitor ne correspond plus à cette connexion catalogue.",
        ));
    }
    let session = app
        .sessions
        .get(&binding.session_id)
        .await
        .ok_or(connection_error(ConnectionError::SessionUnavailable))?;
    let integration = {
        let value = session.lock().await;
        value
            .ensure_account(&value.identity)
            .map_err(connection_error)?;
        let integration = app
            .integrations
            .get(&value.identity)
            .await
            .ok_or(connection_error(ConnectionError::StaleAttempt))?;
        integration
            .lock()
            .await
            .validate_attempt(&value.identity, binding.epoch)
            .map_err(connection_error)?;
        integration
    };
    let (identity, tokens) = app
        .catalog_oidc
        .exchange(
            query.code.unwrap(),
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
    // Recheck after the network exchange: logout/disconnect may have happened
    // while Keycloak was responding. No late callback can resurrect credentials.
    let value = session.lock().await;
    value.ensure_account(&identity).map_err(connection_error)?;
    integration
        .lock()
        .await
        .attach_catalog(&identity, tokens, pending.login.nonce, binding.epoch)
        .map_err(connection_error)?;
    let mut response = Redirect::to("/").into_response();
    set_cookie(&mut response, &app.config, "cm_catalog_login", "", 0);
    Ok(response)
}

// Renewal uses only the account integration, never a browser-session credential.
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

async fn render(app: &App, session: &mut Session, force: bool) -> String {
    if session.expired() {
        session.close();
        return view::welcome();
    }
    let Some(integration) = app.integrations.get(&session.identity).await else {
        return view::dashboard(
            session,
            None,
            &[],
            None,
            &alerts::BudgetRules::default(),
            app.config.allow_unverified_email,
        );
    };
    // Serialize rotation across all sessions of this account. No global map
    // lock is held while contacting Keycloak or the API.
    let mut integration = integration.lock().await;
    let (selections, message) = match fetch_catalog(app, &mut integration, force).await {
        Ok(Some(snapshot)) => (snapshot.selections, None),
        Ok(None) => (vec![], None),
        Err(message) => (vec![], Some(message)),
    };
    view::dashboard(
        session,
        integration.catalog.as_ref(),
        &selections,
        message,
        &integration.budgets,
        app.config.allow_unverified_email,
    )
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
    csrf: String,
    threshold: String,
}

async fn save_alert(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Path(id): Path<u64>,
    Form(input): Form<AlertInput>,
) -> Result<Response, Error> {
    let (_, session) = authorized_session(&app, &headers, &Action { csrf: input.csrf }).await?;
    let value = session.lock().await;
    value
        .ensure_account(&value.identity)
        .map_err(connection_error)?;
    let slot = app.integrations.get(&value.identity).await.ok_or(Error(
        StatusCode::CONFLICT,
        "Connecte d’abord ton catalogue.",
    ))?;
    let mut integration = slot.lock().await;
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
    Ok(Redirect::to("/").into_response())
}

async fn delete_alert(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Path(id): Path<u64>,
    Form(action): Form<Action>,
) -> Result<Response, Error> {
    let (_, session) = authorized_session(&app, &headers, &action).await?;
    let value = session.lock().await;
    value
        .ensure_account(&value.identity)
        .map_err(connection_error)?;
    let slot = app
        .integrations
        .get(&value.identity)
        .await
        .ok_or(Error(StatusCode::NOT_FOUND, "Alerte introuvable."))?;
    if slot.lock().await.budgets.rules.remove(&id).is_none() {
        return Err(Error(StatusCode::NOT_FOUND, "Alerte introuvable."));
    }
    Ok(Redirect::to("/").into_response())
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

async fn home(State(app): State<Arc<App>>, headers: HeaderMap) -> Html<String> {
    if let Some(id) = cookie(&headers, "cm_session")
        && let Some(session) = app.sessions.get(id).await
    {
        return Html(render(&app, &mut *session.lock().await, false).await);
    }
    Html(view::welcome())
}

#[derive(Deserialize)]
struct Action {
    csrf: String,
}

async fn authorized_session(
    app: &App,
    headers: &HeaderMap,
    action: &Action,
) -> Result<(String, Arc<tokio::sync::Mutex<Session>>), Error> {
    if headers.get(header::ORIGIN).and_then(|h| h.to_str().ok()) != Some(app.config.origin.as_str())
    {
        return Err(Error(
            StatusCode::FORBIDDEN,
            "Origine de la requête refusée.",
        ));
    }
    let id =
        cookie(headers, "cm_session").ok_or(Error(StatusCode::UNAUTHORIZED, "Session absente."))?;
    let session = app
        .sessions
        .get(id)
        .await
        .ok_or(Error(StatusCode::UNAUTHORIZED, "Session expirée."))?;
    let value = session.lock().await;
    if value.expired() {
        return Err(connection_error(ConnectionError::SessionUnavailable));
    }
    if !sessions::equal_secret(&value.csrf, &action.csrf) {
        return Err(Error(StatusCode::FORBIDDEN, "Vérification CSRF refusée."));
    }
    drop(value);
    Ok((id.to_owned(), session))
}

async fn refresh(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Form(action): Form<Action>,
) -> Result<Html<String>, Error> {
    let (_, session) = authorized_session(&app, &headers, &action).await?;
    Ok(Html(render(&app, &mut *session.lock().await, true).await))
}

async fn disconnect_catalog(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Form(action): Form<Action>,
) -> Result<Response, Error> {
    let (id, session) = authorized_session(&app, &headers, &action).await?;
    let value = session.lock().await;
    value
        .ensure_account(&value.identity)
        .map_err(connection_error)?;
    if let Some(integration) = app.integrations.get(&value.identity).await {
        integration.lock().await.disconnect_catalog();
    }
    app.sessions.cancel_pending(&id).await;
    let mut response = Redirect::to("/").into_response();
    set_cookie(&mut response, &app.config, "cm_catalog_login", "", 0);
    Ok(response)
}

async fn logout(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Form(action): Form<Action>,
) -> Result<Response, Error> {
    let (id, session) = authorized_session(&app, &headers, &action).await?;
    session.lock().await.close();
    app.sessions.cancel_pending(&id).await;
    {
        let mut pending = app.sessions.pending.lock().await;
        for name in ["cm_app_login", "cm_catalog_login"] {
            if let Some(id) = cookie(&headers, name) {
                pending.remove(id);
            }
        }
    }
    app.sessions.active.write().await.remove(&id);
    let mut response = Redirect::to("/").into_response();
    set_cookie(&mut response, &app.config, "cm_session", "", 0);
    set_cookie(&mut response, &app.config, "cm_app_login", "", 0);
    set_cookie(&mut response, &app.config, "cm_catalog_login", "", 0);
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::oidc::{Identity, Tokens};
    use axum::{Json, body::Body};
    use http_body_util::BodyExt;
    use openidconnect::{AccessToken, Nonce, RefreshToken};
    use serde_json::json;
    use tower::ServiceExt;

    #[derive(Default)]
    struct TestMailer {
        sent: tokio::sync::Mutex<Vec<(String, u64, u64, alerts::Notification)>>,
        fail: std::sync::atomic::AtomicBool,
    }
    impl Mailer for TestMailer {
        fn send<'a>(
            &'a self,
            alert: CostAlert<'a>,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), ()>> + Send + 'a>>
        {
            Box::pin(async move {
                if self.fail.load(std::sync::atomic::Ordering::SeqCst) {
                    return Err(());
                }
                self.sent.lock().await.push((
                    alert.recipient.to_owned(),
                    alert.selection.id,
                    alert.threshold,
                    alert.notification,
                ));
                Ok(())
            })
        }
    }

    fn watched_selection(total: u64) -> serde_json::Value {
        json!([{"id":1,"name":"Montréal","monthly_total_cents":total,
            "services":[{"name":"VM","monthly_price_cents":total}]}])
    }

    async fn add_verified_account(
        app: &App,
    ) -> (String, String, Arc<tokio::sync::Mutex<Integration>>) {
        let identity = Identity {
            issuer: app.config.issuer.clone(),
            subject: "alice".into(),
            name: "Alice".into(),
        };
        let slot = attach_test_integration(app, &identity).await;
        slot.lock()
            .await
            .catalog
            .as_mut()
            .unwrap()
            .tokens
            .as_mut()
            .unwrap()
            .recipient = Some(alerts::Recipient {
            address: "alice@example.test".into(),
            verified: true,
        });
        let session = Session::new(identity);
        let csrf = session.csrf.clone();
        let id = app.sessions.insert(session).await.unwrap();
        (id, csrf, slot)
    }

    #[tokio::test]
    async fn budget_forms_require_session_csrf_ownership_valid_amount_and_account_email() {
        let data = Arc::new(tokio::sync::RwLock::new(watched_selection(2500)));
        let (app, router, server, _) =
            fixture_full(None, Arc::new(TestMailer::default()), data).await;
        let (id, csrf, slot) = add_verified_account(&app).await;
        for (who, token, path, value, expected) in [
            (
                "unknown",
                csrf.as_str(),
                "/alerts/1",
                "20.00",
                StatusCode::UNAUTHORIZED,
            ),
            (
                id.as_str(),
                "forged",
                "/alerts/1",
                "20.00",
                StatusCode::FORBIDDEN,
            ),
            (
                id.as_str(),
                csrf.as_str(),
                "/alerts/999",
                "20.00",
                StatusCode::NOT_FOUND,
            ),
            (
                id.as_str(),
                csrf.as_str(),
                "/alerts/1",
                "-1",
                StatusCode::BAD_REQUEST,
            ),
            (
                id.as_str(),
                csrf.as_str(),
                "/alerts/1",
                "20.00",
                StatusCode::SEE_OTHER,
            ),
        ] {
            let response = router
                .clone()
                .oneshot(
                    axum::http::Request::builder()
                        .method("POST")
                        .uri(path)
                        .header(header::ORIGIN, "http://localhost:8082")
                        .header(header::COOKIE, format!("cm_session={who}"))
                        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                        .body(Body::from(format!("csrf={token}&threshold={value}")))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), expected);
        }
        assert_eq!(slot.lock().await.budgets.rules[&1].threshold_cents, 2000);
        slot.lock()
            .await
            .catalog
            .as_mut()
            .unwrap()
            .tokens
            .as_mut()
            .unwrap()
            .recipient
            .as_mut()
            .unwrap()
            .verified = false;
        assert!(valid_recipient(&*slot.lock().await, false).is_none());
        assert_eq!(
            valid_recipient(&*slot.lock().await, true),
            Some("alice@example.test")
        );
        let bob = Session::new(Identity {
            issuer: app.config.issuer.clone(),
            subject: "bob".into(),
            name: "Bob".into(),
        });
        let bob_csrf = bob.csrf.clone();
        let bob_id = app.sessions.insert(bob).await.unwrap();
        assert_eq!(
            action_request(&router, &bob_id, &bob_csrf, "/alerts/1/delete")
                .await
                .status(),
            StatusCode::NOT_FOUND
        );
        assert!(slot.lock().await.budgets.rules.contains_key(&1));
        assert_eq!(
            action_request(&router, &id, &csrf, "/alerts/1/delete")
                .await
                .status(),
            StatusCode::SEE_OTHER
        );
        assert!(slot.lock().await.budgets.rules.is_empty());
        server.abort();
    }

    #[tokio::test]
    async fn worker_does_not_send_recovery_without_a_delivered_exceedance() {
        let data = Arc::new(tokio::sync::RwLock::new(watched_selection(1500)));
        let mailer = Arc::new(TestMailer::default());
        let (app, _, server, _) = fixture_full(None, mailer.clone(), data.clone()).await;
        let (_, _, slot) = add_verified_account(&app).await;
        {
            let mut entry = slot.lock().await;
            entry.budgets.synchronize("test-instance", &[1]);
            entry.budgets.configure(1, 2000).unwrap();
        }
        check_once(app.clone()).await;
        *data.write().await = watched_selection(2500);
        mailer.fail.store(true, std::sync::atomic::Ordering::SeqCst);
        check_once(app.clone()).await;
        *data.write().await = watched_selection(1500);
        mailer
            .fail
            .store(false, std::sync::atomic::Ordering::SeqCst);
        check_once(app.clone()).await;
        assert!(mailer.sent.lock().await.is_empty());
        server.abort();
    }

    #[tokio::test]
    async fn worker_runs_after_logout_sends_once_rearms_and_retries_failed_delivery() {
        let data = Arc::new(tokio::sync::RwLock::new(watched_selection(2500)));
        let mailer = Arc::new(TestMailer::default());
        let (app, router, server, _) = fixture_full(None, mailer.clone(), data.clone()).await;
        let (id, csrf, slot) = add_verified_account(&app).await;
        {
            let mut entry = slot.lock().await;
            entry.budgets.synchronize("test-instance", &[1]);
            entry.budgets.configure(1, 2000).unwrap();
        }
        assert_eq!(
            action_request(&router, &id, &csrf, "/logout")
                .await
                .status(),
            StatusCode::SEE_OTHER
        );
        check_once(app.clone()).await;
        check_once(app.clone()).await;
        assert_eq!(
            mailer.sent.lock().await.as_slice(),
            &[(
                "alice@example.test".into(),
                1,
                2000,
                alerts::Notification::Exceeded
            )]
        );
        *data.write().await = serde_json::Value::Null;
        check_once(app.clone()).await;
        *data.write().await = watched_selection(3000);
        check_once(app.clone()).await;
        assert_eq!(mailer.sent.lock().await.len(), 1); // Failed reads must not rearm.
        *data.write().await = watched_selection(2000);
        mailer.fail.store(true, std::sync::atomic::Ordering::SeqCst);
        check_once(app.clone()).await;
        assert_eq!(mailer.sent.lock().await.len(), 1);
        mailer
            .fail
            .store(false, std::sync::atomic::Ordering::SeqCst);
        check_once(app.clone()).await;
        check_once(app.clone()).await;
        assert_eq!(mailer.sent.lock().await.len(), 2);
        assert_eq!(
            mailer.sent.lock().await[1],
            (
                "alice@example.test".into(),
                1,
                2000,
                alerts::Notification::Recovered
            )
        );
        *data.write().await = watched_selection(2500);
        check_once(app.clone()).await;
        assert_eq!(mailer.sent.lock().await.len(), 3);
        assert_eq!(
            mailer.sent.lock().await[2].3,
            alerts::Notification::Exceeded
        );
        slot.lock().await.budgets.configure(1, 1900).unwrap();
        mailer.fail.store(true, std::sync::atomic::Ordering::SeqCst);
        check_once(app.clone()).await;
        assert_eq!(mailer.sent.lock().await.len(), 3);
        mailer
            .fail
            .store(false, std::sync::atomic::Ordering::SeqCst);
        check_once(app.clone()).await;
        assert_eq!(mailer.sent.lock().await.len(), 4);
        slot.lock().await.budgets.configure(1, 1900).unwrap();
        check_once(app.clone()).await;
        assert_eq!(mailer.sent.lock().await.len(), 4);
        *data.write().await = json!([]);
        check_once(app.clone()).await;
        assert!(slot.lock().await.budgets.rules.is_empty());
        slot.lock().await.budgets.configure(1, 10).unwrap();
        slot.lock().await.catalog.as_mut().unwrap().expires = Instant::now();
        *data.write().await = watched_selection(2500);
        check_once(app.clone()).await;
        assert!(slot.lock().await.catalog.as_ref().unwrap().tokens.is_none());
        assert_eq!(mailer.sent.lock().await.len(), 4);
        server.abort();
    }

    async fn fixture() -> (Arc<App>, Router, tokio::task::JoinHandle<()>) {
        let (app, router, server, _) = fixture_with_refresh(None).await;
        (app, router, server)
    }

    async fn fixture_with_refresh(
        refresh: Option<serde_json::Value>,
    ) -> (
        Arc<App>,
        Router,
        tokio::task::JoinHandle<()>,
        Arc<std::sync::atomic::AtomicUsize>,
    ) {
        fixture_full(
            refresh,
            Arc::new(TestMailer::default()),
            Arc::new(tokio::sync::RwLock::new(json!([]))),
        )
        .await
    }

    async fn fixture_full(
        refresh: Option<serde_json::Value>,
        mailer: Arc<dyn Mailer>,
        selections: Arc<tokio::sync::RwLock<serde_json::Value>>,
    ) -> (
        Arc<App>,
        Router,
        tokio::task::JoinHandle<()>,
        Arc<std::sync::atomic::AtomicUsize>,
    ) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let metadata = json!({ "issuer":base,"authorization_endpoint":format!("{base}/auth"),
            "token_endpoint":format!("{base}/token"),"jwks_uri":format!("{base}/jwks"),
            "response_types_supported":["code"],"subject_types_supported":["public"],
            "id_token_signing_alg_values_supported":["RS256"] });
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counter = calls.clone();
        let mock = Router::new()
            .route(
                "/.well-known/openid-configuration",
                get(move || {
                    let m = metadata.clone();
                    async { Json(m) }
                }),
            )
            .route("/jwks", get(|| async { Json(json!({"keys":[]})) }))
            .route(
                "/token",
                post(move || {
                    let counter = counter.clone();
                    let refresh = refresh.clone();
                    async move {
                        counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        match refresh {
                            Some(response) => (StatusCode::OK, Json(response)),
                            None => (
                                StatusCode::BAD_REQUEST,
                                Json(json!({"error":"invalid_grant"})),
                            ),
                        }
                    }
                }),
            )
            .route(
                "/api/selections",
                get(move || {
                    let selections = selections.clone();
                    async move {
                        let value = selections.read().await.clone();
                        (
                            if value.is_null() {
                                StatusCode::SERVICE_UNAVAILABLE
                            } else {
                                StatusCode::OK
                            },
                            [("x-catalog-instance", "test-instance")],
                            Json(value),
                        )
                    }
                }),
            );
        let server = tokio::spawn(async move {
            axum::serve(listener, mock).await.unwrap();
        });
        let oidc = Oidc::discover(
            &base,
            &base,
            "http://localhost:8082",
            "client-secret-never-rendered".into(),
            Purpose::Catalog,
        )
        .await
        .unwrap();
        let login_oidc = Oidc::discover(
            &base,
            &base,
            "http://localhost:8082",
            "login-secret-never-rendered".into(),
            Purpose::AppLogin,
        )
        .await
        .unwrap();
        let state = Arc::new(App {
            login_oidc,
            catalog_oidc: oidc,
            config: Config {
                origin: "http://localhost:8082".into(),
                issuer: base.clone(),
                api: base,
                allow_unverified_email: false,
            },
            sessions: Sessions::default(),
            integrations: Integrations::default(),
            http: reqwest::Client::new(),
            mailer,
        });
        (state.clone(), router(state), server, calls)
    }

    #[tokio::test]
    async fn application_login_has_pkce_but_no_catalogue_or_offline_scope_and_cannot_be_replayed() {
        let (_, router, server) = fixture().await;
        let response = router
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .uri("/login")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        let location =
            reqwest::Url::parse(response.headers()[header::LOCATION].to_str().unwrap()).unwrap();
        let query: std::collections::HashMap<_, _> = location.query_pairs().collect();
        assert_eq!(query["code_challenge_method"], "S256");
        assert!(!query["scope"].contains("offline_access"));
        assert!(!query["scope"].contains("selections:read"));
        assert_eq!(query["client_id"], "cost-monitor-login");
        assert_eq!(query["prompt"], "login");
        assert!(
            response.headers()["content-security-policy"]
                .to_str()
                .unwrap()
                .contains(&format!(
                    "form-action 'self' {}",
                    location.origin().ascii_serialization()
                ))
        );
        assert!(!query.contains_key("client_secret"));
        assert_eq!(
            query["redirect_uri"],
            "http://localhost:8082/oidc/login/callback"
        );
        let cookie = response.headers()[header::SET_COOKIE]
            .to_str()
            .unwrap()
            .to_owned();
        assert!(cookie.contains("HttpOnly") && cookie.contains("SameSite=Lax"));
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        assert_eq!(response.headers()["referrer-policy"], "strict-origin");
        for state in ["forged", query["state"].as_ref()] {
            let response = router
                .clone()
                .oneshot(
                    axum::http::Request::builder()
                        .uri(format!("/oidc/login/callback?state={state}&code=forged"))
                        .header(header::COOKIE, cookie.split(';').next().unwrap())
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::BAD_REQUEST);
            let bytes = response.into_body().collect().await.unwrap().to_bytes();
            assert!(!String::from_utf8_lossy(&bytes).contains("client-secret-never-rendered"));
        }
        server.abort();
    }

    async fn action_request(router: &Router, id: &str, csrf: &str, path: &str) -> Response {
        router
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri(path)
                    .header(header::ORIGIN, "http://localhost:8082")
                    .header(header::COOKIE, format!("cm_session={id}"))
                    .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                    .body(Body::from(format!("csrf={csrf}")))
                    .unwrap(),
            )
            .await
            .unwrap()
    }

    async fn attach_test_integration(
        app: &App,
        identity: &Identity,
    ) -> Arc<tokio::sync::Mutex<Integration>> {
        let slot = app.integrations.get_or_create(identity).await.unwrap();
        let epoch = slot.lock().await.begin_catalog();
        slot.lock()
            .await
            .attach_catalog(
                identity,
                Tokens {
                    recipient: None,
                    access: AccessToken::new("private-access".into()),
                    offline: RefreshToken::new("private-offline".into()),
                    lifetime: Duration::from_secs(60),
                },
                Nonce::new_random(),
                epoch,
            )
            .unwrap();
        slot
    }

    fn home_request(id: &str) -> axum::http::Request<Body> {
        axum::http::Request::builder()
            .uri("/")
            .header(header::COOKIE, format!("cm_session={id}"))
            .body(Body::empty())
            .unwrap()
    }

    #[tokio::test]
    async fn expiration_and_relogin_restore_only_the_account_integration_and_disconnect_affects_all_sessions()
     {
        let (app, router, server) = fixture().await;
        let identity = Identity {
            issuer: app.config.issuer.clone(),
            subject: "alice".into(),
            name: "Alice".into(),
        };
        let integration = attach_test_integration(&app, &identity).await;
        let old_id = app
            .sessions
            .insert(Session::new(identity.clone()))
            .await
            .unwrap();
        app.sessions
            .get(&old_id)
            .await
            .unwrap()
            .lock()
            .await
            .created = Instant::now() - SESSION_TTL;
        assert!(app.sessions.get(&old_id).await.is_none());
        assert!(
            integration
                .lock()
                .await
                .catalog
                .as_ref()
                .unwrap()
                .tokens
                .is_some()
        );
        let first = Session::new(identity.clone());
        let first_csrf = first.csrf.clone();
        let first_id = app.sessions.insert(first).await.unwrap();
        let second = Session::new(identity.clone());
        let second_csrf = second.csrf.clone();
        let second_id = app.sessions.insert(second).await.unwrap();
        for id in [&first_id, &second_id] {
            let response = router.clone().oneshot(home_request(id)).await.unwrap();
            let html = String::from_utf8(
                response
                    .into_body()
                    .collect()
                    .await
                    .unwrap()
                    .to_bytes()
                    .to_vec(),
            )
            .unwrap();
            assert!(html.contains("Renouveler maintenant"));
        }
        for other in [
            Identity {
                issuer: identity.issuer.clone(),
                subject: "bob".into(),
                name: "Bob".into(),
            },
            Identity {
                issuer: "https://other.test".into(),
                subject: "alice".into(),
                name: "Other Alice".into(),
            },
        ] {
            let id = app.sessions.insert(Session::new(other)).await.unwrap();
            let response = router.clone().oneshot(home_request(&id)).await.unwrap();
            let html = String::from_utf8(
                response
                    .into_body()
                    .collect()
                    .await
                    .unwrap()
                    .to_bytes()
                    .to_vec(),
            )
            .unwrap();
            assert!(html.contains("Connecter mon catalogue"));
            assert!(!html.contains("Renouveler maintenant"));
        }
        let pending = action_request(&router, &first_id, &first_csrf, "/catalog/connect").await;
        let location =
            reqwest::Url::parse(pending.headers()[header::LOCATION].to_str().unwrap()).unwrap();
        let state = location
            .query_pairs()
            .find(|(k, _)| k == "state")
            .unwrap()
            .1
            .into_owned();
        let pending_cookie = pending.headers()[header::SET_COOKIE]
            .to_str()
            .unwrap()
            .split(';')
            .next()
            .unwrap();
        assert_eq!(
            action_request(&router, &second_id, &second_csrf, "/catalog/disconnect")
                .await
                .status(),
            StatusCode::SEE_OTHER
        );
        assert!(integration.lock().await.catalog.is_none());
        assert!(app.sessions.get(&first_id).await.is_some());
        assert!(app.sessions.get(&second_id).await.is_some());
        let late = router
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .uri(format!("/oidc/catalog/callback?state={state}&code=forged"))
                    .header(
                        header::COOKIE,
                        format!("cm_session={first_id}; {pending_cookie}"),
                    )
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(late.status(), StatusCode::CONFLICT);
        let id = app.sessions.insert(Session::new(identity)).await.unwrap();
        let response = router.oneshot(home_request(&id)).await.unwrap();
        let html = String::from_utf8(
            response
                .into_body()
                .collect()
                .await
                .unwrap()
                .to_bytes()
                .to_vec(),
        )
        .unwrap();
        assert!(html.contains("Connecter mon catalogue"));
        assert!(!html.contains("Renouveler maintenant"));
        server.abort();
    }

    #[tokio::test]
    async fn concurrent_browser_sessions_refresh_the_shared_offline_token_only_once() {
        let (app, router, server, calls) = fixture_with_refresh(Some(json!({
            "access_token":"next-access", "refresh_token":"rotated-offline", "token_type":"Bearer",
            "expires_in":60, "scope":"openid selections:read offline_access"
        })))
        .await;
        let identity = Identity {
            issuer: app.config.issuer.clone(),
            subject: "alice".into(),
            name: "Alice".into(),
        };
        let integration = attach_test_integration(&app, &identity).await;
        integration.lock().await.catalog.as_mut().unwrap().expires = Instant::now();
        let a = app
            .sessions
            .insert(Session::new(identity.clone()))
            .await
            .unwrap();
        let b = app.sessions.insert(Session::new(identity)).await.unwrap();
        let (a, b) = tokio::join!(
            router.clone().oneshot(home_request(&a)),
            router.oneshot(home_request(&b))
        );
        assert_eq!(a.unwrap().status(), StatusCode::OK);
        assert_eq!(b.unwrap().status(), StatusCode::OK);
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
        let value = integration.lock().await;
        let connection = value.catalog.as_ref().unwrap();
        assert_eq!(connection.renewals, 1);
        assert_eq!(
            connection.tokens.as_ref().unwrap().offline.secret(),
            "rotated-offline"
        );
        server.abort();
    }

    #[tokio::test]
    async fn catalogue_authorization_is_optional_session_bound_and_cancelled_by_disconnect_or_logout()
     {
        let (app, router, server) = fixture().await;
        assert_eq!(
            action_request(&router, "unknown", "forged", "/catalog/connect")
                .await
                .status(),
            StatusCode::UNAUTHORIZED
        );
        let session = Session::new(Identity {
            issuer: app.config.issuer.clone(),
            subject: "alice".into(),
            name: "Alice".into(),
        });
        let csrf = session.csrf.clone();
        let id = app.sessions.insert(session).await.unwrap();
        let response = router
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .uri("/")
                    .header(header::COOKIE, format!("cm_session={id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let html = String::from_utf8(
            response
                .into_body()
                .collect()
                .await
                .unwrap()
                .to_bytes()
                .to_vec(),
        )
        .unwrap();
        assert!(html.contains("Connecter mon catalogue"));
        assert!(!html.contains("Renouveler maintenant"));
        for cancel in ["/catalog/disconnect", "/logout"] {
            let response = action_request(&router, &id, &csrf, "/catalog/connect").await;
            assert_eq!(response.status(), StatusCode::SEE_OTHER);
            let url = reqwest::Url::parse(response.headers()[header::LOCATION].to_str().unwrap())
                .unwrap();
            let query: std::collections::HashMap<_, _> = url.query_pairs().collect();
            assert_eq!(query["client_id"], "cost-monitor");
            assert_eq!(
                query["redirect_uri"],
                "http://localhost:8082/oidc/catalog/callback"
            );
            assert!(query["scope"].contains("offline_access"));
            assert!(query["scope"].contains("selections:read"));
            let pending_cookie = response.headers()[header::SET_COOKIE]
                .to_str()
                .unwrap()
                .split(';')
                .next()
                .unwrap()
                .to_owned();
            let pending_id = pending_cookie.split_once('=').unwrap().1;
            let pending = app.sessions.pending.lock().await;
            assert_eq!(pending[pending_id].binding.as_ref().unwrap().session_id, id);
            drop(pending);
            assert_eq!(
                action_request(&router, &id, &csrf, cancel).await.status(),
                StatusCode::SEE_OTHER
            );
            assert!(app.sessions.pending.lock().await.is_empty());
            let callback = router
                .clone()
                .oneshot(
                    axum::http::Request::builder()
                        .uri(format!(
                            "/oidc/catalog/callback?state={}&code=forged",
                            query["state"]
                        ))
                        .header(header::COOKIE, format!("cm_session={id}; {pending_cookie}"))
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(callback.status(), StatusCode::BAD_REQUEST);
            if cancel == "/catalog/disconnect" {
                let session = app.sessions.get(&id).await.unwrap();
                let owner = session.lock().await.identity.clone();
                assert!(
                    app.integrations
                        .get(&owner)
                        .await
                        .unwrap()
                        .lock()
                        .await
                        .catalog
                        .is_none()
                );
                assert!(!session.lock().await.expired());
            }
        }
        assert!(app.sessions.get(&id).await.is_none());
        server.abort();
    }

    #[tokio::test]
    async fn catalogue_revocation_does_not_close_the_application_session() {
        let (app, _, server) = fixture().await;
        let mut session = Session::new(Identity {
            issuer: app.config.issuer.clone(),
            subject: "alice".into(),
            name: "Alice".into(),
        });
        let integration = attach_test_integration(&app, &session.identity).await;
        let html = render(&app, &mut session, true).await;
        assert!(!session.expired());
        assert!(
            integration
                .lock()
                .await
                .catalog
                .as_ref()
                .unwrap()
                .tokens
                .is_none()
        );
        assert!(html.contains("Connecté en tant que Alice"));
        assert!(html.contains("Reconnecter mon catalogue"));
        assert!(!html.contains("private-access") && !html.contains("private-offline"));
        server.abort();
    }

    #[tokio::test]
    async fn catalogue_callback_cannot_be_reused_as_login_or_attached_to_another_session() {
        let (app, router, server) = fixture().await;
        let alice = Session::new(Identity {
            issuer: app.config.issuer.clone(),
            subject: "alice".into(),
            name: "Alice".into(),
        });
        let csrf = alice.csrf.clone();
        let alice_id = app.sessions.insert(alice).await.unwrap();
        let bob_id = app
            .sessions
            .insert(Session::new(Identity {
                issuer: app.config.issuer.clone(),
                subject: "bob".into(),
                name: "Bob".into(),
            }))
            .await
            .unwrap();
        let response = action_request(&router, &alice_id, &csrf, "/catalog/connect").await;
        let location =
            reqwest::Url::parse(response.headers()[header::LOCATION].to_str().unwrap()).unwrap();
        let state = location
            .query_pairs()
            .find(|(k, _)| k == "state")
            .unwrap()
            .1
            .into_owned();
        let pending_cookie = response.headers()[header::SET_COOKIE]
            .to_str()
            .unwrap()
            .split(';')
            .next()
            .unwrap();
        let login = router
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .uri(format!("/oidc/login/callback?state={state}&code=forged"))
                    .header(
                        header::COOKIE,
                        format!("cm_session={alice_id}; {pending_cookie}"),
                    )
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(login.status(), StatusCode::BAD_REQUEST);
        let transferred = router
            .oneshot(
                axum::http::Request::builder()
                    .uri(format!("/oidc/catalog/callback?state={state}&code=forged"))
                    .header(
                        header::COOKIE,
                        format!("cm_session={bob_id}; {pending_cookie}"),
                    )
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(transferred.status(), StatusCode::UNAUTHORIZED);
        let alice = app
            .sessions
            .get(&alice_id)
            .await
            .unwrap()
            .lock()
            .await
            .identity
            .clone();
        let bob = app
            .sessions
            .get(&bob_id)
            .await
            .unwrap()
            .lock()
            .await
            .identity
            .clone();
        assert!(
            app.integrations
                .get(&alice)
                .await
                .unwrap()
                .lock()
                .await
                .catalog
                .is_none()
        );
        assert!(app.integrations.get(&bob).await.is_none());
        server.abort();
    }

    #[tokio::test]
    async fn actions_require_session_origin_and_csrf_and_logout_preserves_account_integration() {
        let (app, router, server) = fixture().await;
        let session = Session::new(Identity {
            issuer: app.config.issuer.clone(),
            subject: "alice".into(),
            name: "<Alice>".into(),
        });
        let identity = session.identity.clone();
        let integration = attach_test_integration(&app, &identity).await;
        let csrf = session.csrf.clone();
        let id = app.sessions.insert(session).await.unwrap();
        let reference = app.sessions.get(&id).await.unwrap();
        for (origin, body, status) in [
            (
                "http://evil.invalid",
                format!("csrf={csrf}"),
                StatusCode::FORBIDDEN,
            ),
            (
                "http://localhost:8082",
                "csrf=forged".into(),
                StatusCode::FORBIDDEN,
            ),
        ] {
            let response = router
                .clone()
                .oneshot(
                    axum::http::Request::builder()
                        .method("POST")
                        .uri("/catalog/refresh")
                        .header(header::ORIGIN, origin)
                        .header(header::COOKIE, format!("cm_session={id}"))
                        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                        .body(Body::from(body))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), status);
        }
        let response = router
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .uri("/")
                    .header(header::COOKIE, format!("cm_session={id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let html = String::from_utf8(
            response
                .into_body()
                .collect()
                .await
                .unwrap()
                .to_bytes()
                .to_vec(),
        )
        .unwrap();
        assert!(html.contains("&lt;Alice&gt;"));
        assert!(!html.contains("private-access") && !html.contains("private-offline"));
        let response = router
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/logout")
                    .header(header::ORIGIN, "http://localhost:8082")
                    .header(header::COOKIE, format!("cm_session={id}"))
                    .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                    .body(Body::from(format!("csrf={csrf}")))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert!(
            response.headers()[header::SET_COOKIE]
                .to_str()
                .unwrap()
                .contains("Max-Age=0")
        );
        assert!(app.sessions.get(&id).await.is_none());
        assert!(
            integration
                .lock()
                .await
                .catalog
                .as_ref()
                .unwrap()
                .tokens
                .is_some()
        );
        assert!(reference.lock().await.expired());
        let fresh_session = Session::new(identity.clone());
        let fresh_id = app.sessions.insert(fresh_session).await.unwrap();
        assert_ne!(id, fresh_id);
        let restored = app.integrations.get(&identity).await.unwrap();
        assert!(Arc::ptr_eq(&integration, &restored));
        let fresh = app.sessions.get(&fresh_id).await.unwrap();
        let html = render(&app, &mut *fresh.lock().await, false).await;
        assert!(html.contains("Renouveler maintenant"));
        assert!(!html.contains("Connecter mon catalogue"));
        assert!(!html.contains("private-access") && !html.contains("private-offline"));
        server.abort();
    }
}

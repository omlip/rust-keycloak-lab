use super::*;
use crate::{
    oidc::{Identity, Tokens},
    test_issuer,
};
use axum::body::Body;
use http_body_util::BodyExt;
use openidconnect::{AccessToken, Nonce, RefreshToken};
use serde_json::{Value, json};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use tokio::sync::{Mutex, Notify, RwLock};
use tower::ServiceExt;

#[derive(Default)]
struct TestMailer {
    sent: Mutex<Vec<alerts::Notification>>,
    fail: AtomicBool,
}
impl Mailer for TestMailer {
    fn send<'a>(
        &'a self,
        alert: CostAlert<'a>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), ()>> + Send + 'a>> {
        Box::pin(async move {
            if self.fail.load(Ordering::SeqCst) {
                return Err(());
            }
            assert_eq!(alert.recipient, "alice@example.test");
            self.sent.lock().await.push(alert.notification);
            Ok(())
        })
    }
}
#[derive(Default)]
struct Gate {
    enabled: AtomicBool,
    started: Notify,
    release: Notify,
}
struct Fixture {
    app: Arc<App>,
    router: Router,
    server: tokio::task::JoinHandle<()>,
    response: Arc<RwLock<Option<Value>>>,
    data: Arc<RwLock<Value>>,
    mailer: Arc<TestMailer>,
    calls: Arc<AtomicUsize>,
    gate: Arc<Gate>,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.server.abort();
    }
}
fn selections(total: u64) -> Value {
    json!([{"id":1,"name":"<Sélection>","monthly_total_cents":total,"services":[{"name":"<VM>","monthly_price_cents":total}]}])
}
async fn fixture() -> Fixture {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let metadata = json!({"issuer":base,"authorization_endpoint":format!("{base}/auth"),"token_endpoint":format!("{base}/token"),
        "jwks_uri":format!("{base}/jwks"),"response_types_supported":["code"],"subject_types_supported":["public"],"id_token_signing_alg_values_supported":["RS256"]});
    let response: Arc<RwLock<Option<Value>>> = Arc::new(RwLock::new(None));
    let data = Arc::new(RwLock::new(selections(2500)));
    let calls = Arc::new(AtomicUsize::new(0));
    let gate = Arc::new(Gate::default());
    let (r, d, c, g) = (response.clone(), data.clone(), calls.clone(), gate.clone());
    let mock = Router::new()
        .route(
            "/.well-known/openid-configuration",
            get(move || async move { Json(metadata) }),
        )
        .route("/jwks", get(|| async { Json(test_issuer::jwks()) }))
        .route(
            "/token",
            post(move || {
                let (r, c, g) = (r.clone(), c.clone(), g.clone());
                async move {
                    c.fetch_add(1, Ordering::SeqCst);
                    if g.enabled.load(Ordering::SeqCst) {
                        g.started.notify_one();
                        g.release.notified().await;
                    }
                    match r.read().await.clone() {
                        Some(value) => (StatusCode::OK, Json(value)),
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
                let d = d.clone();
                async move {
                    let value = d.read().await.clone();
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
    let server = tokio::spawn(async move { axum::serve(listener, mock).await.unwrap() });
    let catalog_oidc = Oidc::discover(
        &base,
        &base,
        "http://localhost:8082",
        "catalog-secret-never-rendered".into(),
    )
    .await
    .unwrap();
    let auth = Authenticator::discover(&base, &base).await.unwrap();
    let mailer = Arc::new(TestMailer::default());
    let app = Arc::new(App {
        auth,
        catalog_oidc,
        config: Config {
            origin: "http://localhost:8082".into(),
            issuer: base.clone(),
            api: base,
            allow_unverified_email: false,
        },
        sessions: Sessions::default(),
        integrations: Integrations::default(),
        http: reqwest::Client::new(),
        mailer: mailer.clone(),
    });
    Fixture {
        router: router(app.clone()),
        app,
        server,
        response,
        data,
        mailer,
        calls,
        gate,
    }
}
fn jwt(f: &Fixture, subject: &str, sid: &str) -> String {
    test_issuer::sign(test_issuer::claims(&f.app.config.issuer, subject, sid))
}
async fn request(
    f: &Fixture,
    subject: &str,
    sid: &str,
    method: &str,
    path: &str,
    body: &str,
    cookie: Option<&str>,
) -> Response {
    let mut request = axum::http::Request::builder()
        .method(method)
        .uri(path)
        .header(
            header::AUTHORIZATION,
            format!("Bearer {}", jwt(f, subject, sid)),
        )
        .header(header::ACCEPT, "application/json")
        .header(header::ORIGIN, &f.app.config.origin)
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded");
    if let Some(cookie) = cookie {
        request = request.header(header::COOKIE, cookie);
    }
    f.router
        .clone()
        .oneshot(request.body(Body::from(body.to_owned())).unwrap())
        .await
        .unwrap()
}
async fn body(response: Response) -> Value {
    serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap()
}
async fn attach(f: &Fixture, subject: &str) -> Arc<Mutex<Integration>> {
    let identity = Identity {
        issuer: f.app.config.issuer.clone(),
        subject: subject.into(),
        name: subject.into(),
    };
    let slot = f.app.integrations.get_or_create(&identity).await.unwrap();
    let epoch = slot.lock().await.begin_catalog();
    slot.lock()
        .await
        .attach_catalog(
            &identity,
            Tokens {
                access: AccessToken::new("private-access".into()),
                offline: RefreshToken::new("private-offline".into()),
                lifetime: Duration::from_secs(60),
                recipient: Some(alerts::Recipient {
                    address: "alice@example.test".into(),
                    verified: true,
                }),
            },
            Nonce::new_random(),
            epoch,
        )
        .unwrap();
    slot
}
async fn begin(f: &Fixture, subject: &str, sid: &str) -> (String, reqwest::Url) {
    let response = request(f, subject, sid, "POST", "/api/catalog/connect", "", None).await;
    assert_eq!(response.status(), StatusCode::OK);
    let cookie = response.headers()[header::SET_COOKIE]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    let value = body(response).await;
    (
        cookie,
        reqwest::Url::parse(value["url"].as_str().unwrap()).unwrap(),
    )
}
async fn stage(f: &Fixture, cookie: &str, url: &reqwest::Url) -> Response {
    let state = url
        .query_pairs()
        .find(|(k, _)| k == "state")
        .unwrap()
        .1
        .into_owned();
    f.router
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .uri(format!(
                    "/oidc/catalog/callback?state={state}&code=test-code"
                ))
                .header(header::COOKIE, cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap()
}
async fn exchange_response(f: &Fixture, subject: &str, url: &reqwest::Url) {
    let nonce = url
        .query_pairs()
        .find(|(k, _)| k == "nonce")
        .unwrap()
        .1
        .into_owned();
    let mut claims = test_issuer::claims(&f.app.config.issuer, subject, "oidc-session");
    claims["aud"] = json!("cost-monitor");
    claims["iat"] = json!(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
    );
    claims["nonce"] = json!(nonce);
    claims["email"] = json!("alice@example.test");
    claims["email_verified"] = json!(true);
    *f.response.write().await = Some(
        json!({"access_token":"private-access","refresh_token":"private-offline","id_token":test_issuer::sign(claims),
        "token_type":"Bearer","expires_in":60,"scope":"openid profile email selections:read offline_access"}),
    );
}

#[tokio::test]
async fn api_requires_a_verified_bearer_never_a_cookie_and_exposes_only_account_data() {
    let f = fixture().await;
    for path in [
        "/api/dashboard",
        "/api/catalog/connect",
        "/api/catalog/complete",
        "/api/logout",
        "/api/alerts/1",
    ] {
        let response = f
            .router
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .uri(path)
                    .method(if path == "/api/dashboard" {
                        "GET"
                    } else {
                        "POST"
                    })
                    .header(header::COOKIE, "cm_session=old; cm_catalog_login=old")
                    .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                    .body(Body::from("threshold=1"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(response.headers()[header::WWW_AUTHENTICATE], "Bearer");
    }
    attach(&f, "alice").await;
    *f.data.write().await = selections(u64::MAX);
    let response = request(&f, "alice", "a", "GET", "/api/dashboard", "", None).await;
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    let value = body(response).await;
    assert_eq!(value["name"], "alice");
    assert_eq!(
        value["selections"][0]["monthly_total_cents"],
        u64::MAX.to_string()
    );
    assert!(value.get("csrf").is_none());
    assert!(
        !value.to_string().contains("private-access")
            && !value.to_string().contains("private-offline")
    );
    let bob = body(request(&f, "bob", "b", "GET", "/api/dashboard", "", None).await).await;
    assert_eq!(bob["connected"], false);
    assert_eq!(bob["selections"], json!([]));
    for path in ["/login", "/logout", "/oidc/login/callback"] {
        assert_eq!(
            request(&f, "alice", "a", "GET", path, "", None)
                .await
                .status(),
            StatusCode::NOT_FOUND
        );
    }
}

#[tokio::test]
async fn consent_is_pkce_offline_explicit_browser_bound_and_completed_only_by_the_same_bearer_session()
 {
    let f = fixture().await;
    let (cookie, url) = begin(&f, "alice", "a").await;
    let params: std::collections::HashMap<_, _> = url.query_pairs().collect();
    assert_eq!(params["client_id"], "cost-monitor");
    assert_eq!(params["code_challenge_method"], "S256");
    assert_eq!(params["prompt"], "consent");
    assert!(
        params["scope"].contains("offline_access") && params["scope"].contains("selections:read")
    );
    assert_eq!(
        stage(&f, &cookie, &url).await.status(),
        StatusCode::SEE_OTHER
    );
    assert_eq!(f.calls.load(Ordering::SeqCst), 0); // Callback GET cannot attach or exchange.
    let response = request(
        &f,
        "alice",
        "other-sid",
        "POST",
        "/api/catalog/complete",
        "",
        Some(&cookie),
    )
    .await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert_eq!(f.calls.load(Ordering::SeqCst), 0);
    let (cookie, url) = begin(&f, "alice", "a").await;
    exchange_response(&f, "alice", &url).await;
    stage(&f, &cookie, &url).await;
    assert_eq!(
        request(
            &f,
            "alice",
            "a",
            "POST",
            "/api/catalog/complete",
            "",
            Some(&cookie)
        )
        .await
        .status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(f.calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        request(
            &f,
            "alice",
            "a",
            "POST",
            "/api/catalog/complete",
            "",
            Some(&cookie)
        )
        .await
        .status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(f.calls.load(Ordering::SeqCst), 1); // Replay is a no-op, not a second exchange.
    let data = body(request(&f, "alice", "a", "GET", "/api/dashboard", "", None).await).await;
    assert_eq!(data["connected"], true);
    let (cookie, url) = begin(&f, "alice", "a").await;
    exchange_response(&f, "bob", &url).await;
    stage(&f, &cookie, &url).await;
    assert_eq!(
        request(
            &f,
            "alice",
            "a",
            "POST",
            "/api/catalog/complete",
            "",
            Some(&cookie)
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        body(request(&f, "alice", "a", "GET", "/api/dashboard", "", None).await).await["name"],
        "alice"
    );
}

#[tokio::test]
async fn catalogue_exchange_requires_signature_nonce_and_granted_offline_scope() {
    let f = fixture().await;
    for failure in ["nonce", "signature", "scope"] {
        let (cookie, url) = begin(&f, "alice", "a").await;
        let mut response_url = url.clone();
        if failure == "nonce" {
            let params: Vec<_> = url
                .query_pairs()
                .map(|(k, v)| (k.into_owned(), v.into_owned()))
                .collect();
            response_url
                .query_pairs_mut()
                .clear()
                .extend_pairs(params.iter().map(|(k, v)| {
                    (
                        k.as_str(),
                        if k == "nonce" {
                            "wrong-nonce"
                        } else {
                            v.as_str()
                        },
                    )
                }));
        }
        exchange_response(&f, "alice", &response_url).await;
        {
            let mut response = f.response.write().await;
            let response = response.as_mut().unwrap();
            if failure == "scope" {
                response["scope"] = json!("openid selections:read");
            }
            if failure == "signature" {
                response["id_token"] = json!("forged.jwt.signature");
            }
        }
        stage(&f, &cookie, &url).await;
        assert_eq!(
            request(
                &f,
                "alice",
                "a",
                "POST",
                "/api/catalog/complete",
                "",
                Some(&cookie)
            )
            .await
            .status(),
            StatusCode::BAD_REQUEST,
            "accepted invalid {failure}"
        );
        assert_eq!(
            body(request(&f, "alice", "a", "GET", "/api/dashboard", "", None).await).await["connected"],
            false
        );
    }
}

#[tokio::test]
async fn forged_expired_cancelled_and_replayed_callbacks_never_exchange_credentials() {
    let f = fixture().await;
    for query in [
        "state=forged&code=x",
        "error=access_denied",
        "state=forged&iss=http://foreign&code=x",
    ] {
        let (cookie, _) = begin(&f, "alice", "a").await;
        let response = f
            .router
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .uri(format!("/oidc/catalog/callback?{query}"))
                    .header(header::COOKIE, cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }
    let (cookie, url) = begin(&f, "alice", "a").await;
    f.app
        .sessions
        .pending
        .lock()
        .await
        .values_mut()
        .for_each(|p| p.created = Instant::now() - LOGIN_TTL);
    assert_eq!(
        stage(&f, &cookie, &url).await.status(),
        StatusCode::BAD_REQUEST
    );
    let (cookie, url) = begin(&f, "alice", "a").await;
    assert_eq!(
        stage(&f, &cookie, &url).await.status(),
        StatusCode::SEE_OTHER
    );
    assert_eq!(
        stage(&f, &cookie, &url).await.status(),
        StatusCode::BAD_REQUEST
    );
    let (cookie, url) = begin(&f, "alice", "a").await;
    request(
        &f,
        "alice",
        "a",
        "POST",
        "/api/catalog/disconnect",
        "",
        None,
    )
    .await;
    assert_eq!(
        stage(&f, &cookie, &url).await.status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(f.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn logout_or_disconnect_during_token_exchange_prevents_late_attachment() {
    for action in ["/api/logout", "/api/catalog/disconnect"] {
        let f = fixture().await;
        let (cookie, url) = begin(&f, "alice", "a").await;
        exchange_response(&f, "alice", &url).await;
        stage(&f, &cookie, &url).await;
        f.gate.enabled.store(true, Ordering::SeqCst);
        let router = f.router.clone();
        let jwt = jwt(&f, "alice", "a");
        let complete = tokio::spawn(async move {
            router
                .oneshot(
                    axum::http::Request::builder()
                        .method("POST")
                        .uri("/api/catalog/complete")
                        .header(header::AUTHORIZATION, format!("Bearer {jwt}"))
                        .header(header::ORIGIN, "http://localhost:8082")
                        .header(header::COOKIE, cookie)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap()
        });
        f.gate.started.notified().await;
        assert_eq!(
            request(&f, "alice", "a", "POST", action, "", None)
                .await
                .status(),
            StatusCode::NO_CONTENT
        );
        f.gate.release.notify_one();
        let status = complete.await.unwrap().status();
        assert!(status == StatusCode::UNAUTHORIZED || status == StatusCode::CONFLICT);
        let identity = Identity {
            issuer: f.app.config.issuer.clone(),
            subject: "alice".into(),
            name: "alice".into(),
        };
        assert!(
            f.app
                .integrations
                .get(&identity)
                .await
                .unwrap()
                .lock()
                .await
                .catalog
                .is_none()
        );
    }
}

#[tokio::test]
async fn mutations_require_origin_identity_ownership_exact_amount_and_authorized_email() {
    let f = fixture().await;
    let slot = attach(&f, "alice").await;
    let response = f
        .router
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/api/alerts/1")
                .header(
                    header::AUTHORIZATION,
                    format!("Bearer {}", jwt(&f, "alice", "a")),
                )
                .header(header::ORIGIN, "http://evil.invalid")
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(Body::from("threshold=1"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert_eq!(
        request(&f, "bob", "b", "POST", "/api/alerts/1", "threshold=1", None)
            .await
            .status(),
        StatusCode::CONFLICT
    );
    assert_eq!(
        request(
            &f,
            "alice",
            "a",
            "POST",
            "/api/alerts/999",
            "threshold=1",
            None
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );
    for value in ["-1", "NaN", "1.234", "18446744073709551616"] {
        assert_eq!(
            request(
                &f,
                "alice",
                "a",
                "POST",
                "/api/alerts/1",
                &format!("threshold={value}"),
                None
            )
            .await
            .status(),
            StatusCode::BAD_REQUEST
        );
    }
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
    assert_eq!(
        request(
            &f,
            "alice",
            "a",
            "POST",
            "/api/alerts/1",
            "threshold=10",
            None
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST
    );
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
        .verified = true;
    assert_eq!(
        request(
            &f,
            "alice",
            "a",
            "POST",
            "/api/alerts/1",
            "threshold=10%2C25",
            None
        )
        .await
        .status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(slot.lock().await.budgets.rules[&1].threshold_cents, 1025);
    assert_eq!(
        request(&f, "alice", "a", "POST", "/api/alerts/1/delete", "", None)
            .await
            .status(),
        StatusCode::NO_CONTENT
    );
    assert!(slot.lock().await.budgets.rules.is_empty());
}

#[tokio::test]
async fn offline_worker_survives_spa_logout_retries_and_rearms_only_after_recovery() {
    let f = fixture().await;
    let slot = attach(&f, "alice").await;
    request(
        &f,
        "alice",
        "a",
        "POST",
        "/api/alerts/1",
        "threshold=10",
        None,
    )
    .await;
    let (cookie, url) = begin(&f, "alice", "a").await;
    request(&f, "alice", "a", "POST", "/api/logout", "", None).await;
    assert_eq!(
        stage(&f, &cookie, &url).await.status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        request(&f, "alice", "a", "GET", "/api/dashboard", "", None)
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );
    f.mailer.fail.store(true, Ordering::SeqCst);
    check_once(f.app.clone()).await;
    assert!(f.mailer.sent.lock().await.is_empty());
    f.mailer.fail.store(false, Ordering::SeqCst);
    check_once(f.app.clone()).await;
    check_once(f.app.clone()).await;
    assert_eq!(
        *f.mailer.sent.lock().await,
        vec![alerts::Notification::Exceeded]
    );
    *f.data.write().await = Value::Null;
    check_once(f.app.clone()).await;
    *f.data.write().await = selections(1000);
    check_once(f.app.clone()).await;
    check_once(f.app.clone()).await;
    assert_eq!(
        *f.mailer.sent.lock().await,
        vec![
            alerts::Notification::Exceeded,
            alerts::Notification::Recovered
        ]
    );
    let fresh = body(
        request(
            &f,
            "alice",
            "fresh-sso-session",
            "GET",
            "/api/dashboard",
            "",
            None,
        )
        .await,
    )
    .await;
    assert_eq!(fresh["selections"][0]["threshold_cents"], "1000");
    assert!(slot.lock().await.catalog.is_some());
    *f.data.write().await = json!([]);
    check_once(f.app.clone()).await;
    assert!(slot.lock().await.budgets.rules.is_empty());
}

#[tokio::test]
async fn concurrent_spa_sessions_rotate_the_shared_offline_token_once_and_revocation_only_affects_catalogue()
 {
    let f = fixture().await;
    let slot = attach(&f, "alice").await;
    slot.lock().await.catalog.as_mut().unwrap().expires = Instant::now();
    *f.response.write().await = Some(
        json!({"access_token":"next-access","refresh_token":"rotated-offline","token_type":"Bearer","expires_in":60,"scope":"openid selections:read offline_access"}),
    );
    let (a, b) = tokio::join!(
        request(&f, "alice", "a", "GET", "/api/dashboard", "", None),
        request(&f, "alice", "b", "GET", "/api/dashboard", "", None)
    );
    assert_eq!(a.status(), StatusCode::OK);
    assert_eq!(b.status(), StatusCode::OK);
    assert_eq!(f.calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        slot.lock()
            .await
            .catalog
            .as_ref()
            .unwrap()
            .tokens
            .as_ref()
            .unwrap()
            .offline
            .secret(),
        "rotated-offline"
    );
    *f.response.write().await = None;
    let data =
        body(request(&f, "alice", "a", "POST", "/api/catalog/refresh", "", None).await).await;
    assert_eq!(data["connected"], false);
    assert_eq!(data["has_catalog"], true);
    assert_eq!(data["name"], "alice");
}

mod support;
use axum::{
    body::Body,
    http::{Request, StatusCode, header},
};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use support::*;
use tower::ServiceExt;

async fn request(
    f: &Fixture,
    method: &str,
    path: &str,
    claims: &Value,
    body: Option<Value>,
) -> axum::response::Response {
    let mut req = Request::builder()
        .method(method)
        .uri(path)
        .header(header::AUTHORIZATION, format!("Bearer {}", token(claims)));
    let body = if let Some(body) = body {
        req = req.header(header::CONTENT_TYPE, "application/json");
        Body::from(body.to_string())
    } else {
        Body::empty()
    };
    f.app
        .clone()
        .oneshot(req.body(body).unwrap())
        .await
        .unwrap()
}

#[tokio::test]
async fn cost_monitor_reads_only_its_users_selections_even_with_admin_roles_and_excess_scopes() {
    let f = Fixture::new().await;
    let created = request(
        &f,
        "POST",
        "/api/selections",
        &claims_for("alice", &["users"]),
        Some(json!({"name":"Private"})),
    )
    .await;
    assert_eq!(created.status(), StatusCode::CREATED);
    let body: Value =
        serde_json::from_slice(&created.into_body().collect().await.unwrap().to_bytes()).unwrap();
    let path = format!("/api/selections/{}", body["id"]);
    for roles in [
        &["users"][..],
        &["admin"][..],
        &["users", "admin", "catalog-seed"][..],
    ] {
        let mut delegated = claims_for("alice", roles);
        delegated["azp"] = json!("cost-monitor");
        delegated["scope"] = json!(
            "selections:read selections:write catalog:read catalog:write catalog:seed offline_access"
        );
        assert_eq!(
            request(&f, "GET", "/api/selections", &delegated, None)
                .await
                .status(),
            StatusCode::OK
        );
        assert_eq!(
            request(&f, "GET", &path, &delegated, None).await.status(),
            StatusCode::OK
        );
        for (method, uri, input) in [
            (
                "POST",
                "/api/selections".to_owned(),
                Some(json!({"name":"Attack"})),
            ),
            ("PATCH", path.clone(), Some(json!({"name":"Attack"}))),
            ("DELETE", path.clone(), None),
            ("PUT", format!("{path}/services/1"), None),
            ("DELETE", format!("{path}/services/1"), None),
            ("GET", "/api/catalog/services".into(), None),
            (
                "POST",
                "/api/catalog/services".into(),
                Some(
                    json!({"name":"VM","category":"compute","description":"","monthly_price_cents":1}),
                ),
            ),
            (
                "PUT",
                "/api/catalog/services/1".into(),
                Some(
                    json!({"name":"VM","category":"compute","description":"","monthly_price_cents":1}),
                ),
            ),
            ("DELETE", "/api/catalog/services/1".into(), None),
            ("POST", "/api/catalog/seed".into(), None),
        ] {
            assert_eq!(
                request(&f, method, &uri, &delegated, input).await.status(),
                StatusCode::FORBIDDEN,
                "{method} {uri}"
            );
        }
        delegated["sub"] = json!("bob");
        assert_eq!(
            request(&f, "GET", &path, &delegated, None).await.status(),
            StatusCode::NOT_FOUND
        );
    }
    // Exercise shared test helpers and JWKS caching in this integration binary too.
    assert_eq!(
        f.bearer("/api/selections", &token(&claims(&["users"])))
            .await
            .status(),
        StatusCode::OK
    );
    assert!(f.calls.load(std::sync::atomic::Ordering::SeqCst) > 0);
}

#[tokio::test]
async fn missing_scope_unknown_client_or_missing_client_never_grants_permissions() {
    let f = Fixture::new().await;
    for (client, scope) in [
        (
            "cost-monitor-login",
            "catalog:read catalog:write selections:read selections:write offline_access",
        ),
        ("olaninc-spa", ""),
        ("unknown", "selections:read selections:write"),
        ("", "selections:read"),
    ] {
        let mut claims = claims(&["admin"]);
        claims["azp"] = json!(client);
        claims["scope"] = json!(scope);
        assert_eq!(
            request(&f, "GET", "/api/selections", &claims, None)
                .await
                .status(),
            StatusCode::FORBIDDEN
        );
    }
}

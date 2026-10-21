mod support;

use axum::{
    body::Body,
    http::{Request, StatusCode, header},
    response::Response,
};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use std::sync::atomic::Ordering;
use support::*;
use tower::ServiceExt;

const SERVICES: &str = "/api/catalog/services";
const SEED: &str = "/api/catalog/seed";

fn service_input() -> Value {
    json!({ "name": "VM", "category": "compute", "description": "Une VM fictive", "monthly_price_cents": 2500 })
}

async fn request(
    fixture: &Fixture,
    method: &str,
    path: &str,
    roles: Option<&[&str]>,
    body: Option<Value>,
) -> Response {
    let mut request = Request::builder().method(method).uri(path);
    if let Some(roles) = roles {
        request = request.header(
            header::AUTHORIZATION,
            format!("Bearer {}", token(&claims(roles))),
        );
    }
    let body = if let Some(body) = body {
        request = request.header(header::CONTENT_TYPE, "application/json");
        Body::from(body.to_string())
    } else {
        Body::empty()
    };
    fixture
        .app
        .clone()
        .oneshot(request.body(body).unwrap())
        .await
        .unwrap()
}

async fn json_body(response: Response) -> Value {
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

#[tokio::test]
async fn catalog_starts_empty_and_is_shared_by_both_roles() {
    let fixture = Fixture::new().await;
    for roles in [&["users"][..], &["admin"][..]] {
        let response = fixture.bearer(SERVICES, &token(&claims(roles))).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(json_body(response).await, json!([]));
    }
    assert_eq!(fixture.calls.load(Ordering::SeqCst), 1);
    let created = json_body(
        request(
            &fixture,
            "POST",
            SERVICES,
            Some(&["admin"]),
            Some(service_input()),
        )
        .await,
    )
    .await;
    let path = format!("{SERVICES}/{}", created["id"]);
    let response = fixture.bearer(&path, &token(&claims(&["users"]))).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(json_body(response).await, created);
}

#[tokio::test]
async fn all_catalog_endpoints_require_authentication() {
    let fixture = Fixture::new().await;
    for (method, path) in [
        ("GET", SERVICES),
        ("GET", "/api/catalog/services/1"),
        ("POST", SERVICES),
        ("PUT", "/api/catalog/services/1"),
        ("DELETE", "/api/catalog/services/1"),
        ("POST", SEED),
    ] {
        let response = request(&fixture, method, path, None, None).await;
        assert_eq!(
            response.status(),
            StatusCode::UNAUTHORIZED,
            "{method} {path}"
        );
        assert_eq!(response.headers()[header::WWW_AUTHENTICATE], "Bearer");
        assert!(!response.headers().contains_key(header::LOCATION));
    }
}

#[tokio::test]
async fn users_cannot_mutate_or_seed_even_with_missing_json() {
    let fixture = Fixture::new().await;
    for (method, path) in [
        ("POST", SERVICES),
        ("PUT", "/api/catalog/services/1"),
        ("DELETE", "/api/catalog/services/1"),
        ("POST", SEED),
    ] {
        assert_eq!(
            request(&fixture, method, path, Some(&["users"]), None)
                .await
                .status(),
            StatusCode::FORBIDDEN
        );
    }
    assert_eq!(
        json_body(request(&fixture, "GET", SERVICES, Some(&["users"]), None).await).await,
        json!([])
    );
}

#[tokio::test]
async fn valid_token_without_app_roles_cannot_read_catalog() {
    let fixture = Fixture::new().await;
    assert_eq!(
        fixture
            .bearer(SERVICES, &token(&claims(&[])))
            .await
            .status(),
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn admin_can_create_update_and_delete_and_missing_services_return_404() {
    let fixture = Fixture::new().await;
    let created = request(
        &fixture,
        "POST",
        SERVICES,
        Some(&["admin"]),
        Some(service_input()),
    )
    .await;
    assert_eq!(created.status(), StatusCode::CREATED);
    let created = json_body(created).await;
    assert!(created["id"].as_u64().unwrap() > 0);
    let path = format!("{SERVICES}/{}", created["id"]);
    let mut input = service_input();
    input["monthly_price_cents"] = json!(3999);
    input["category"] = json!("network");
    let response = request(&fixture, "PUT", &path, Some(&["admin"]), Some(input)).await;
    assert_eq!(response.status(), StatusCode::OK);
    let updated = json_body(response).await;
    assert_eq!(updated["id"], created["id"]);
    assert_eq!(updated["monthly_price_cents"], 3999);
    assert_eq!(updated["category"], "network");
    let response = request(&fixture, "DELETE", &path, Some(&["admin"]), None).await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert!(
        response
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .is_empty()
    );
    for method in ["GET", "PUT", "DELETE"] {
        assert_eq!(
            request(
                &fixture,
                method,
                &path,
                Some(&["admin"]),
                Some(service_input())
            )
            .await
            .status(),
            StatusCode::NOT_FOUND
        );
    }
}

#[tokio::test]
async fn seed_replaces_everything_and_does_not_accumulate_duplicates_or_reuse_ids() {
    let fixture = Fixture::new().await;
    let original = json_body(
        request(
            &fixture,
            "POST",
            SERVICES,
            Some(&["admin"]),
            Some(service_input()),
        )
        .await,
    )
    .await;
    for _ in 0..2 {
        let response = request(&fixture, "POST", SEED, Some(&["catalog-seed"]), None).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(json_body(response).await, json!({ "count": 5 }));
        let services =
            json_body(request(&fixture, "GET", SERVICES, Some(&["users"]), None).await).await;
        assert_eq!(services.as_array().unwrap().len(), 5);
        assert!(
            services
                .as_array()
                .unwrap()
                .iter()
                .all(|service| service["id"] != original["id"])
        );
        assert!(
            services
                .as_array()
                .unwrap()
                .iter()
                .any(|service| service["name"] == "Stockage objet")
        );
    }
}

#[tokio::test]
async fn seed_requires_its_dedicated_role_not_a_human_admin_role() {
    let fixture = Fixture::new().await;
    for roles in [
        &["admin"][..],
        &["users"][..],
        &["admin", "users"][..],
        &[][..],
    ] {
        assert_eq!(
            request(&fixture, "POST", SEED, Some(roles), None)
                .await
                .status(),
            StatusCode::FORBIDDEN
        );
    }
    assert_eq!(
        json_body(request(&fixture, "GET", SERVICES, Some(&["admin"]), None).await).await,
        json!([])
    );
    let response = request(&fixture, "POST", SEED, Some(&["catalog-seed"]), None).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(json_body(response).await, json!({ "count": 5 }));
}

#[tokio::test]
async fn catalog_loader_role_cannot_read_or_manage_other_resources() {
    let fixture = Fixture::new().await;
    for (method, path) in [
        ("GET", SERVICES),
        ("GET", "/api/catalog/services/1"),
        ("POST", SERVICES),
        ("PUT", "/api/catalog/services/1"),
        ("DELETE", "/api/catalog/services/1"),
        ("GET", "/api/user"),
        ("GET", "/api/admin"),
    ] {
        assert_eq!(
            request(
                &fixture,
                method,
                path,
                Some(&["catalog-seed"]),
                Some(service_input())
            )
            .await
            .status(),
            StatusCode::FORBIDDEN,
            "{method} {path}"
        );
    }
}

#[tokio::test]
async fn raw_api_key_or_client_secret_is_not_an_api_credential() {
    let fixture = Fixture::new().await;
    let request = Request::builder()
        .method("POST")
        .uri(SEED)
        .header("X-API-Key", "a-client-secret-is-not-an-access-token")
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        fixture.app.clone().oneshot(request).await.unwrap().status(),
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn malformed_types_or_unknown_category_are_rejected_without_business_validation() {
    let fixture = Fixture::new().await;
    for (field, value) in [
        ("category", json!("unknown")),
        ("monthly_price_cents", json!(-1)),
        ("monthly_price_cents", json!(1.5)),
    ] {
        let mut input = service_input();
        input[field] = value;
        assert_eq!(
            request(&fixture, "POST", SERVICES, Some(&["admin"]), Some(input))
                .await
                .status(),
            StatusCode::BAD_REQUEST
        );
    }
    let empty =
        json!({ "name": "", "category": "storage", "description": "", "monthly_price_cents": 0 });
    for _ in 0..2 {
        assert_eq!(
            request(
                &fixture,
                "POST",
                SERVICES,
                Some(&["admin"]),
                Some(empty.clone())
            )
            .await
            .status(),
            StatusCode::CREATED
        );
    }
}

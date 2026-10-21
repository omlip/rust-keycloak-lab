mod support;

use axum::{
    body::Body,
    http::{Request, StatusCode, header},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use http_body_util::BodyExt;
use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
use serde_json::{Value, json};
use std::{
    sync::atomic::Ordering,
    time::{SystemTime, UNIX_EPOCH},
};
use support::*;
use tower::ServiceExt;

#[tokio::test]
async fn absent_or_malformed_authorization_returns_json_401_without_redirect() {
    let fixture = Fixture::new().await;
    for path in ["/api/user", "/api/admin"] {
        for value in [
            None,
            Some("Basic dXNlcjpwYXNz"),
            Some("Bearer garbage"),
            Some("Bearer"),
        ] {
            let response = fixture.get(path, value).await;
            assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
            assert_eq!(response.headers()[header::WWW_AUTHENTICATE], "Bearer");
            assert_eq!(response.headers()[header::CONTENT_TYPE], "application/json");
            assert!(!response.headers().contains_key(header::LOCATION));
        }
    }
    assert_eq!(fixture.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn users_can_only_access_user_endpoint() {
    let fixture = Fixture::new().await;
    let token = token(&claims(&["users"]));
    let response = fixture.bearer("/api/user", &token).await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(
        serde_json::from_slice::<Value>(&body).unwrap()["message"],
        "Bienvenue dans l’espace utilisateur"
    );
    assert_eq!(
        fixture.bearer("/api/admin", &token).await.status(),
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn admin_without_users_role_can_access_both_endpoints() {
    let fixture = Fixture::new().await;
    let token = token(&claims(&["admin"]));
    for path in ["/api/user", "/api/admin"] {
        assert_eq!(fixture.bearer(path, &token).await.status(), StatusCode::OK);
    }
}

#[tokio::test]
async fn no_roles_or_unrelated_roles_return_403() {
    let fixture = Fixture::new().await;
    let mut no_roles = claims(&[]);
    no_roles.as_object_mut().unwrap().remove("realm_access");
    for claims in [
        no_roles,
        claims(&["realm-admin"]),
        claims(&["administrator"]),
    ] {
        for path in ["/api/user", "/api/admin"] {
            assert_eq!(
                fixture.bearer(path, &token(&claims)).await.status(),
                StatusCode::FORBIDDEN
            );
        }
    }
}

#[tokio::test]
async fn invalid_claims_are_rejected_before_role_authorization() {
    let fixture = Fixture::new().await;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    for (field, value) in [
        ("exp", json!(now - 1)),
        ("iss", json!("http://attacker.test/realms/olaninc")),
        ("aud", json!("another-api")),
        ("aud", json!(["account", "olaninc-spa"])),
        ("nbf", json!(now + 3600)),
        ("typ", json!("ID")),
        ("typ", json!("Refresh")),
        ("sub", json!("")),
    ] {
        let mut claims = claims(&["admin"]);
        claims[field] = value;
        assert_eq!(
            fixture.bearer("/api/admin", &token(&claims)).await.status(),
            StatusCode::UNAUTHORIZED,
            "invalid {field}"
        );
    }
    for field in ["exp", "iss", "aud", "sub", "typ"] {
        let mut claims = claims(&["admin"]);
        claims.as_object_mut().unwrap().remove(field);
        assert_eq!(
            fixture.bearer("/api/admin", &token(&claims)).await.status(),
            StatusCode::UNAUTHORIZED,
            "missing {field}"
        );
    }
}

#[tokio::test]
async fn audience_can_be_a_list_containing_the_api() {
    let fixture = Fixture::new().await;
    let mut claims = claims(&["users"]);
    claims["aud"] = json!(["account", AUDIENCE]);
    assert_eq!(
        fixture.bearer("/api/user", &token(&claims)).await.status(),
        StatusCode::OK
    );
}

#[tokio::test]
async fn forged_signature_and_modified_payload_return_401() {
    let fixture = Fixture::new().await;
    let mut forged = token(&claims(&["admin"])).into_bytes();
    let position = forged.iter().rposition(|byte| *byte == b'.').unwrap() + 5;
    forged[position] = if forged[position] == b'A' { b'B' } else { b'A' };
    assert_eq!(
        fixture
            .bearer("/api/admin", std::str::from_utf8(&forged).unwrap())
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );

    let signed = token(&claims(&["users"]));
    let mut parts = signed.split('.').map(String::from).collect::<Vec<_>>();
    parts[1] = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims(&["admin"])).unwrap());
    assert_eq!(
        fixture
            .bearer("/api/admin", &parts.join("."))
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn wrong_algorithm_unknown_kid_and_missing_kid_return_401() {
    let fixture = Fixture::new().await;
    let symmetric = encode(
        &Header::new(Algorithm::HS256),
        &claims(&["admin"]),
        &EncodingKey::from_secret(b"not-a-public-key"),
    )
    .unwrap();
    for token in [
        symmetric,
        sign(&claims(&["admin"]), None),
        sign(&claims(&["admin"]), Some("unknown")),
    ] {
        assert_eq!(
            fixture.bearer("/api/admin", &token).await.status(),
            StatusCode::UNAUTHORIZED
        );
    }
}

#[tokio::test]
async fn keys_are_cached_and_unknown_kids_do_not_flood_keycloak() {
    let fixture = Fixture::new().await;
    let valid = token(&claims(&["users"]));
    let unknown = sign(&claims(&["admin"]), Some("unknown"));
    for _ in 0..5 {
        assert_eq!(
            fixture.bearer("/api/user", &valid).await.status(),
            StatusCode::OK
        );
        assert_eq!(
            fixture.bearer("/api/admin", &unknown).await.status(),
            StatusCode::UNAUTHORIZED
        );
    }
    assert_eq!(fixture.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn unavailable_or_empty_jwks_fails_closed_with_503() {
    let fixture = Fixture::with_keys(json!({ "keys": [] })).await;
    assert_eq!(
        fixture
            .bearer("/api/admin", &token(&claims(&["admin"])))
            .await
            .status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    // Retrying immediately is throttled, including failures.
    assert_eq!(
        fixture
            .bearer("/api/admin", &token(&claims(&["admin"])))
            .await
            .status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    assert_eq!(fixture.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn encryption_key_is_not_accepted_as_a_signature_key() {
    let mut jwk = signing_material().jwk.clone();
    jwk["use"] = json!("enc");
    let fixture = Fixture::with_keys(json!({ "keys": [jwk] })).await;
    assert_eq!(
        fixture
            .bearer("/api/admin", &token(&claims(&["admin"])))
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn duplicate_authorization_headers_return_401() {
    let fixture = Fixture::new().await;
    let authorization = format!("Bearer {}", token(&claims(&["admin"])));
    let request = Request::builder()
        .uri("/api/admin")
        .header(header::AUTHORIZATION, &authorization)
        .header(header::AUTHORIZATION, &authorization)
        .body(Body::empty())
        .unwrap();
    let response = fixture.app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn health_endpoint_is_public() {
    let fixture = Fixture::new().await;
    assert_eq!(fixture.get("/health", None).await.status(), StatusCode::OK);
}

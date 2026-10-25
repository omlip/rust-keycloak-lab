mod support;

use axum::{
    body::Body,
    http::{Request, StatusCode, header},
    response::Response,
};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use support::*;
use tower::ServiceExt;

const URL: &str = "/api/selections";

#[tokio::test]
async fn backend_instance_is_stable_for_one_router_but_changes_for_a_new_store_lifetime() {
    let a = Fixture::new().await;
    let b = Fixture::new().await;
    let token = token(&claims(&["users"]));
    let first = a.bearer(URL, &token).await;
    let again = a.bearer(URL, &token).await;
    let other = b.bearer(URL, &token).await;
    assert_eq!(
        first.headers()["x-catalog-instance"],
        again.headers()["x-catalog-instance"]
    );
    assert_ne!(
        first.headers()["x-catalog-instance"],
        other.headers()["x-catalog-instance"]
    );
}

async fn request(
    f: &Fixture,
    method: &str,
    path: &str,
    actor: Option<(&str, &[&str])>,
    body: Option<Value>,
) -> Response {
    let mut req = Request::builder().method(method).uri(path);
    if let Some((subject, roles)) = actor {
        req = req.header(
            header::AUTHORIZATION,
            format!("Bearer {}", token(&claims_for(subject, roles))),
        );
    }
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

async fn json_body(response: Response) -> Value {
    serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap()
}

async fn create(f: &Fixture, subject: &str) -> Value {
    let response = request(
        f,
        "POST",
        URL,
        Some((subject, &["users"])),
        Some(json!({"name":"Montréal"})),
    )
    .await;
    assert_eq!(response.status(), StatusCode::CREATED);
    json_body(response).await
}

async fn catalog_service(f: &Fixture, price: u64) -> Value {
    json_body(request(f, "POST", "/api/catalog/services", Some(("catalog-admin", &["admin"])),
        Some(json!({"name":"VM", "category":"compute", "description":"Demo", "monthly_price_cents":price}))).await).await
}

#[tokio::test]
async fn every_endpoint_requires_a_valid_token_and_an_application_role() {
    let f = Fixture::new().await;
    let valid = token(&claims(&["users"]));
    assert_eq!(f.bearer(URL, &valid).await.status(), StatusCode::OK);
    assert!(f.calls.load(std::sync::atomic::Ordering::SeqCst) > 0);
    for (method, path) in [
        ("GET", URL),
        ("POST", URL),
        ("GET", "/api/selections/1"),
        ("PATCH", "/api/selections/1"),
        ("DELETE", "/api/selections/1"),
        ("PUT", "/api/selections/1/services/1"),
        ("DELETE", "/api/selections/1/services/1"),
    ] {
        let response = request(&f, method, path, None, None).await;
        assert_eq!(
            response.status(),
            StatusCode::UNAUTHORIZED,
            "{method} {path}"
        );
        assert!(response.headers().get(header::LOCATION).is_none());
        for roles in [&[][..], &["catalog-seed"][..], &["realm-admin"][..]] {
            assert_eq!(
                request(&f, method, path, Some(("machine", roles)), None)
                    .await
                    .status(),
                StatusCode::FORBIDDEN
            );
        }
    }
}

#[tokio::test]
async fn same_role_and_admin_never_bypass_ownership_or_reveal_existence() {
    let f = Fixture::new().await;
    let selection = create(&f, "alice").await;
    let id = selection["id"].as_u64().unwrap();
    let own = create(&f, "bob").await;
    for roles in [&["users"][..], &["admin"][..], &["users", "admin"][..]] {
        let actor = Some(("bob", roles));
        let list =
            json_body(request(&f, "GET", "/api/selections?owner=alice", actor, None).await).await;
        assert_eq!(list, json!([own]));
        for (method, suffix, body) in [
            ("GET", "", None),
            ("PATCH", "", Some(json!({"name":"Attack"}))),
            ("DELETE", "", None),
            ("PUT", "/services/999", None),
            ("DELETE", "/services/999", None),
        ] {
            let foreign = request(
                &f,
                method,
                &format!("{URL}/{id}{suffix}"),
                actor,
                body.clone(),
            )
            .await;
            let absent = request(&f, method, &format!("{URL}/999999{suffix}"), actor, body).await;
            assert_eq!(foreign.status(), StatusCode::NOT_FOUND);
            assert_eq!(absent.status(), StatusCode::NOT_FOUND);
            assert_eq!(json_body(foreign).await, json_body(absent).await);
        }
    }
    assert_eq!(
        json_body(
            request(
                &f,
                "GET",
                &format!("{URL}/{id}"),
                Some(("alice", &["users"])),
                None
            )
            .await
        )
        .await,
        selection
    );
}

#[tokio::test]
async fn owner_cannot_be_supplied_or_transferred_and_names_are_validated() {
    let f = Fixture::new().await;
    let actor = Some(("alice", &["users"][..]));
    for input in [
        json!({"name":"Test", "owner":"bob"}),
        json!({"name":"Test", "sub":"bob"}),
        json!({"name":" \n "}),
        json!({"name":12}),
        json!({}),
    ] {
        assert_eq!(
            request(&f, "POST", URL, actor, Some(input)).await.status(),
            StatusCode::BAD_REQUEST
        );
    }
    let selection = create(&f, "alice").await;
    let path = format!("{URL}/{}", selection["id"]);
    assert_eq!(
        request(
            &f,
            "PATCH",
            &path,
            actor,
            Some(json!({"name":"Test", "owner":"bob"}))
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        request(&f, "PATCH", &path, actor, Some(json!({"name":" "})))
            .await
            .status(),
        StatusCode::BAD_REQUEST
    );
    let renamed = json_body(
        request(
            &f,
            "PATCH",
            &path,
            actor,
            Some(json!({"name":" Production "})),
        )
        .await,
    )
    .await;
    assert_eq!(renamed["name"], "Production");
    assert!(renamed.get("owner").is_none());
}

#[tokio::test]
async fn crud_uses_current_prices_without_quantities_or_duplicates() {
    let f = Fixture::new().await;
    let actor = Some(("alice", &["users"][..]));
    let selection = create(&f, "alice").await;
    let path = format!("{URL}/{}", selection["id"]);
    let vm = catalog_service(&f, 1250).await;
    let service_path = format!("{path}/services/{}", vm["id"]);
    for _ in 0..2 {
        let response = request(&f, "PUT", &service_path, actor, None).await;
        assert_eq!(response.status(), StatusCode::OK);
        let details = json_body(response).await;
        assert_eq!(details["services"].as_array().unwrap().len(), 1);
        assert_eq!(details["monthly_total_cents"], 1250);
    }
    assert_eq!(
        request(&f, "PUT", &format!("{path}/services/9999"), actor, None)
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
    let mut updated = vm.clone();
    updated["monthly_price_cents"] = json!(3000);
    request(
        &f,
        "PUT",
        &format!("/api/catalog/services/{}", vm["id"]),
        Some(("admin", &["admin"])),
        Some(updated),
    )
    .await;
    assert_eq!(
        json_body(request(&f, "GET", &path, actor, None).await).await["monthly_total_cents"],
        3000
    );
    let empty = json_body(request(&f, "DELETE", &service_path, actor, None).await).await;
    assert_eq!(empty["monthly_total_cents"], 0);
    assert_eq!(
        request(&f, "DELETE", &path, actor, None).await.status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        request(&f, "GET", &path, actor, None).await.status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        json_body(request(&f, "GET", "/api/catalog/services", actor, None).await)
            .await
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn catalog_deletion_and_seed_clean_memberships_but_keep_named_selections() {
    let f = Fixture::new().await;
    let actor = Some(("alice", &["users"][..]));
    let selection = create(&f, "alice").await;
    let path = format!("{URL}/{}", selection["id"]);
    let vm = catalog_service(&f, 100).await;
    request(
        &f,
        "PUT",
        &format!("{path}/services/{}", vm["id"]),
        actor,
        None,
    )
    .await;
    request(
        &f,
        "DELETE",
        &format!("/api/catalog/services/{}", vm["id"]),
        Some(("admin", &["admin"])),
        None,
    )
    .await;
    assert_eq!(
        json_body(request(&f, "GET", &path, actor, None).await).await,
        selection
    );
    let vm = catalog_service(&f, 200).await;
    request(
        &f,
        "PUT",
        &format!("{path}/services/{}", vm["id"]),
        actor,
        None,
    )
    .await;
    assert_eq!(
        request(
            &f,
            "POST",
            "/api/catalog/seed",
            Some(("loader", &["catalog-seed"])),
            None
        )
        .await
        .status(),
        StatusCode::OK
    );
    assert_eq!(
        json_body(request(&f, "GET", &path, actor, None).await).await,
        selection
    );
    let fresh = Fixture::new().await;
    assert_eq!(
        json_body(request(&fresh, "GET", URL, actor, None).await).await,
        json!([])
    );
}

#[tokio::test]
async fn an_admin_has_the_same_personal_capabilities_not_other_peoples_data() {
    let f = Fixture::new().await;
    let actor = Some(("admin", &["admin"][..]));
    let response = request(
        &f,
        "POST",
        URL,
        actor,
        Some(json!({"name":"Admin personnel"})),
    )
    .await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let id = json_body(response).await["id"].as_u64().unwrap();
    let path = format!("{URL}/{id}");
    let vm = catalog_service(&f, 100).await;
    assert_eq!(
        request(
            &f,
            "PUT",
            &format!("{path}/services/{}", vm["id"]),
            actor,
            None
        )
        .await
        .status(),
        StatusCode::OK
    );
    assert_eq!(
        request(&f, "GET", &path, Some(("other-admin", &["admin"])), None)
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        request(&f, "DELETE", &path, actor, None).await.status(),
        StatusCode::NO_CONTENT
    );
}

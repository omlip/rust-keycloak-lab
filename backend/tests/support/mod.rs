use axum::{
    Json, Router,
    body::Body,
    http::{Request, header},
    routing::get,
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
use olaninc_api::{
    app,
    auth::{AuthConfig, Authenticator},
};
use rand::{SeedableRng, rngs::StdRng};
use rsa::{RsaPrivateKey, pkcs1::EncodeRsaPrivateKey, traits::PublicKeyParts};
use serde_json::{Value, json};
use std::{
    sync::{
        Arc, OnceLock,
        atomic::{AtomicUsize, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};
use tower::ServiceExt;

pub const ISSUER: &str = "http://localhost:8081/realms/olaninc";
pub const AUDIENCE: &str = "olaninc-api";

pub struct SigningMaterial {
    pub key: EncodingKey,
    pub jwk: Value,
}

pub fn signing_material() -> &'static SigningMaterial {
    static KEYS: OnceLock<SigningMaterial> = OnceLock::new();
    KEYS.get_or_init(|| {
        // Deterministic keys for tests only. No private signing key ships in the application.
        let private = RsaPrivateKey::new(&mut StdRng::seed_from_u64(42), 2048).unwrap();
        let der = private.to_pkcs1_der().unwrap();
        SigningMaterial {
            key: EncodingKey::from_rsa_der(der.as_bytes()),
            jwk: json!({
                "kty": "RSA", "kid": "test-key", "alg": "RS256", "use": "sig",
                "n": URL_SAFE_NO_PAD.encode(private.n().to_bytes_be()),
                "e": URL_SAFE_NO_PAD.encode(private.e().to_bytes_be()),
            }),
        }
    })
}

pub fn claims(roles: &[&str]) -> Value {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    json!({
        "sub": "demo-subject", "iss": ISSUER, "aud": AUDIENCE,
        "exp": now + 300, "iat": now, "typ": "Bearer",
        "realm_access": { "roles": roles }
    })
}

pub fn token(claims: &Value) -> String {
    sign(claims, Some("test-key"))
}

pub fn sign(claims: &Value, kid: Option<&str>) -> String {
    let mut header = Header::new(Algorithm::RS256);
    header.kid = kid.map(String::from);
    encode(&header, claims, &signing_material().key).unwrap()
}

pub struct Fixture {
    pub app: Router,
    pub calls: Arc<AtomicUsize>,
    server: tokio::task::JoinHandle<()>,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.server.abort();
    }
}

impl Fixture {
    pub async fn new() -> Self {
        Self::with_keys(json!({ "keys": [signing_material().jwk] })).await
    }

    pub async fn with_keys(keys: Value) -> Self {
        let calls = Arc::new(AtomicUsize::new(0));
        let counter = calls.clone();
        let jwks_app = Router::new().route(
            "/certs",
            get(move || {
                counter.fetch_add(1, Ordering::SeqCst);
                let keys = keys.clone();
                async move { Json(keys) }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/certs", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            axum::serve(listener, jwks_app).await.unwrap();
        });
        let auth = Authenticator::new(AuthConfig {
            issuer: ISSUER.into(),
            audience: AUDIENCE.into(),
            jwks_url: url,
        })
        .unwrap();
        Self {
            app: app(auth),
            calls,
            server,
        }
    }

    pub async fn get(&self, path: &str, authorization: Option<&str>) -> axum::response::Response {
        let mut request = Request::builder().uri(path);
        if let Some(value) = authorization {
            request = request.header(header::AUTHORIZATION, value);
        }
        self.app
            .clone()
            .oneshot(request.body(Body::empty()).unwrap())
            .await
            .unwrap()
    }

    pub async fn bearer(&self, path: &str, token: &str) -> axum::response::Response {
        self.get(path, Some(&format!("Bearer {token}"))).await
    }
}

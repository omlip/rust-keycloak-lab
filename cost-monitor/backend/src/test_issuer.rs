//! Ephemeral RSA fixture, never a deployed key or production authentication bypass.
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use jsonwebtoken::{Algorithm, EncodingKey, Header, encode, jwk::JwkSet};
use rsa::{RsaPrivateKey, pkcs1::EncodeRsaPrivateKey, traits::PublicKeyParts};
use std::{
    sync::OnceLock,
    time::{SystemTime, UNIX_EPOCH},
};

fn key() -> &'static RsaPrivateKey {
    static KEY: OnceLock<RsaPrivateKey> = OnceLock::new();
    KEY.get_or_init(|| RsaPrivateKey::new(&mut rand::rngs::OsRng, 2048).unwrap())
}
pub fn jwks() -> JwkSet {
    serde_json::from_value(serde_json::json!({"keys":[{
        "kty":"RSA", "kid":"test-key", "alg":"RS256", "use":"sig",
        "n":URL_SAFE_NO_PAD.encode(key().n().to_bytes_be()), "e":URL_SAFE_NO_PAD.encode(key().e().to_bytes_be())
    }]})).unwrap()
}
pub fn claims(issuer: &str, subject: &str, sid: &str) -> serde_json::Value {
    serde_json::json!({"iss":issuer,"sub":subject,"sid":sid,"aud":crate::auth::AUDIENCE,
        "azp":crate::auth::CLIENT,"typ":"Bearer","preferred_username":subject,
        "exp":SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs()+300})
}
pub fn sign(claims: serde_json::Value) -> String {
    let mut header = Header::new(Algorithm::RS256);
    header.kid = Some("test-key".into());
    encode(
        &header,
        &claims,
        &EncodingKey::from_rsa_der(key().to_pkcs1_der().unwrap().as_bytes()),
    )
    .unwrap()
}
pub fn headers(token: &str) -> axum::http::HeaderMap {
    let mut headers = axum::http::HeaderMap::new();
    headers.insert(
        axum::http::header::AUTHORIZATION,
        format!("Bearer {token}").parse().unwrap(),
    );
    headers
}

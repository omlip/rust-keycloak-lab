//! Only signed access tokens from the public Cost Monitor SPA authenticate APIs.
use crate::oidc::Identity;
use axum::http::{HeaderMap, header};
use headers::{Authorization, HeaderMapExt, authorization::Bearer};
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode, decode_header, jwk::JwkSet};
use serde::Deserialize;
use std::time::Duration;

pub const CLIENT: &str = "cost-monitor-login";
pub const AUDIENCE: &str = "cost-monitor-api";

pub struct Authenticator {
    keys: JwkSet,
    validation: Validation,
}

#[derive(Clone, Deserialize)]
pub struct Principal {
    pub iss: String,
    pub sub: String,
    pub sid: String,
    pub exp: u64,
    pub aud: serde_json::Value,
    pub azp: String,
    pub typ: String,
    #[serde(default)]
    pub preferred_username: String,
}

impl Principal {
    pub fn identity(&self) -> Identity {
        Identity {
            issuer: self.iss.clone(),
            subject: self.sub.clone(),
            name: if self.preferred_username.is_empty() {
                "Utilisateur".into()
            } else {
                self.preferred_username.clone()
            },
        }
    }
}

impl Authenticator {
    pub fn new(issuer: &str, keys: JwkSet) -> Self {
        let mut validation = Validation::new(Algorithm::RS256);
        validation.set_issuer(&[issuer]);
        validation.set_audience(&[AUDIENCE]);
        validation.set_required_spec_claims(&["iss", "sub", "aud", "exp"]);
        validation.validate_nbf = true;
        validation.leeway = 0;
        Self { keys, validation }
    }

    pub async fn discover(public: &str, internal: &str) -> Result<Self, &'static str> {
        #[derive(Deserialize)]
        struct Metadata {
            issuer: String,
            jwks_uri: String,
        }
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(15))
            .build()
            .map_err(|_| "Invalid JWT transport")?;
        let metadata: Metadata = http
            .get(format!("{internal}/.well-known/openid-configuration"))
            .send()
            .await
            .map_err(|_| "JWT discovery unavailable")?
            .error_for_status()
            .map_err(|_| "JWT discovery refused")?
            .json()
            .await
            .map_err(|_| "Invalid JWT discovery")?;
        if metadata.issuer != public {
            return Err("Unexpected JWT issuer");
        }
        let path = metadata
            .jwks_uri
            .strip_prefix(&format!("{public}/"))
            .ok_or("Unexpected JWKS endpoint")?;
        let keys = http
            .get(format!("{internal}/{path}"))
            .send()
            .await
            .map_err(|_| "JWKS unavailable")?
            .error_for_status()
            .map_err(|_| "JWKS refused")?
            .json()
            .await
            .map_err(|_| "Invalid JWKS")?;
        Ok(Self::new(public, keys))
    }

    pub fn verify(&self, headers: &HeaderMap) -> Result<Principal, &'static str> {
        if headers.get_all(header::AUTHORIZATION).iter().count() != 1 {
            return Err("Access token required");
        }
        let authorization = headers
            .typed_get::<Authorization<Bearer>>()
            .ok_or("Access token required")?;
        let jwt = authorization.token();
        let header = decode_header(jwt).map_err(|_| "Invalid access token")?;
        if header.alg != Algorithm::RS256 {
            return Err("Invalid signing algorithm");
        }
        let kid = header
            .kid
            .filter(|kid| !kid.is_empty())
            .ok_or("Missing signing key")?;
        let jwk = self.keys.find(&kid).ok_or("Unknown signing key")?;
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
            return Err("Invalid signing key usage");
        }
        let key = DecodingKey::from_jwk(jwk).map_err(|_| "Invalid signing key")?;
        let principal = decode::<Principal>(jwt, &key, &self.validation)
            .map_err(|_| "Invalid access token")?
            .claims;
        if principal.typ != "Bearer"
            || principal.azp != CLIENT
            || principal.sub.is_empty()
            || principal.sid.is_empty()
        {
            return Err("Invalid API credential");
        }
        Ok(principal)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_issuer::{claims, headers, jwks, sign};
    #[test]
    fn accepts_only_signed_current_access_tokens_for_this_api_and_client() {
        let issuer = "http://issuer.test/realms/demo";
        let auth = Authenticator::new(issuer, jwks());
        assert_eq!(
            auth.verify(&headers(&sign(claims(issuer, "alice", "session"))))
                .unwrap()
                .sub,
            "alice"
        );
        for (field, value) in [
            ("iss", serde_json::json!("http://other.test")),
            ("aud", serde_json::json!("olaninc-api")),
            ("azp", serde_json::json!("olaninc-spa")),
            ("azp", serde_json::json!("cost-monitor")),
            ("typ", serde_json::json!("ID")),
            ("typ", serde_json::json!("Offline")),
            ("sub", serde_json::json!("")),
            ("sid", serde_json::json!("")),
            ("exp", serde_json::json!(1)),
            ("nbf", serde_json::json!(u64::MAX)),
        ] {
            let mut invalid = claims(issuer, "alice", "session");
            invalid[field] = value;
            assert!(
                auth.verify(&headers(&sign(invalid))).is_err(),
                "accepted {field}"
            );
        }
        let mut invalid = claims(issuer, "alice", "session");
        invalid.as_object_mut().unwrap().remove("sid");
        assert!(auth.verify(&headers(&sign(invalid))).is_err());
        assert!(auth.verify(&HeaderMap::new()).is_err());
        let mut duplicate = headers(&sign(claims(issuer, "alice", "session")));
        duplicate.append(header::AUTHORIZATION, "Bearer duplicate".parse().unwrap());
        assert!(auth.verify(&duplicate).is_err());
        for token in ["forged", "eyJhbGciOiJub25lIn0.e30."] {
            assert!(auth.verify(&headers(token)).is_err());
        }
        let jwt = sign(claims(issuer, "alice", "session"));
        let mut parts: Vec<_> = jwt.split('.').map(str::to_owned).collect();
        parts[2] = "A".repeat(parts[2].len());
        assert!(auth.verify(&headers(&parts.join("."))).is_err());
        let mut cookie_only = HeaderMap::new();
        cookie_only.insert(header::COOKIE, "cm_session=old".parse().unwrap());
        assert!(auth.verify(&cookie_only).is_err());
    }
}

use openidconnect::{
    AccessToken, AccessTokenHash, AsyncHttpClient, AuthenticationFlow, AuthorizationCode, ClientId,
    ClientSecret, CsrfToken, EndpointMaybeSet, EndpointNotSet, EndpointSet, HttpClientError,
    HttpRequest, HttpResponse, IssuerUrl, Nonce, NonceVerifier, OAuth2TokenResponse,
    PkceCodeChallenge, PkceCodeVerifier, RedirectUrl, RefreshToken, RequestTokenError, Scope,
    core::{
        CoreClient, CoreErrorResponseType, CoreProviderMetadata, CoreResponseType,
        CoreTokenResponse,
    },
};
use std::{future::Future, pin::Pin, time::Duration};

type Client = CoreClient<
    EndpointSet,
    EndpointNotSet,
    EndpointNotSet,
    EndpointNotSet,
    EndpointSet,
    EndpointMaybeSet,
>;

pub struct Oidc {
    client: Client,
    http: Backchannel,
    issuer: String,
    purpose: Purpose,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Purpose {
    AppLogin,
    Catalog,
}

impl Purpose {
    pub fn client_id(self) -> &'static str {
        match self {
            Self::AppLogin => "cost-monitor-login",
            Self::Catalog => "cost-monitor",
        }
    }
    pub fn callback(self) -> &'static str {
        match self {
            Self::AppLogin => "/oidc/login/callback",
            Self::Catalog => "/oidc/catalog/callback",
        }
    }
}

// Only transport is rewritten for Docker. Discovery issuer validation, signed ID
// token issuer/audience/expiry/nonce verification and browser URLs remain public.
#[derive(Clone)]
pub struct Backchannel {
    client: reqwest::Client,
    public: String,
    internal: String,
}

impl<'c> AsyncHttpClient<'c> for Backchannel {
    type Error = HttpClientError<reqwest::Error>;
    type Future =
        Pin<Box<dyn Future<Output = Result<HttpResponse, Self::Error>> + Send + Sync + 'c>>;
    fn call(&'c self, mut request: HttpRequest) -> Self::Future {
        Box::pin(async move {
            let uri = request.uri().to_string();
            // Reject unexpected discovery/JWKS/token URLs; never forward a code or
            // client secret to another host or follow redirects.
            let Some(path) = uri.strip_prefix(&(self.public.clone() + "/")) else {
                return Err(HttpClientError::Other("Unexpected OIDC endpoint".into()));
            };
            *request.uri_mut() = format!("{}/{path}", self.internal)
                .parse()
                .map_err(|_| HttpClientError::Other("Invalid OIDC endpoint".into()))?;
            self.client.call(request).await
        })
    }
}

pub struct Login {
    pub url: String,
    pub state: CsrfToken,
    pub nonce: Nonce,
    pub verifier: PkceCodeVerifier,
}
pub struct Tokens {
    pub access: AccessToken,
    pub offline: RefreshToken,
    pub lifetime: Duration,
}
#[derive(Clone)]
pub struct Identity {
    pub issuer: String,
    pub subject: String,
    pub name: String,
}
impl Identity {
    pub fn same_account(&self, other: &Self) -> bool {
        self.issuer == other.issuer && self.subject == other.subject
    }
}
#[derive(Debug, PartialEq, Eq)]
pub enum OidcError {
    InvalidAuthorization,
    Revoked,
    Unavailable,
}

impl Oidc {
    pub async fn discover(
        public: &str,
        internal: &str,
        origin: &str,
        secret: String,
        purpose: Purpose,
    ) -> Result<Self, OidcError> {
        let http = Backchannel {
            client: reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(Duration::from_secs(15))
                .build()
                .map_err(|_| OidcError::Unavailable)?,
            public: public.trim_end_matches('/').into(),
            internal: internal.trim_end_matches('/').into(),
        };
        let issuer = IssuerUrl::new(public.into()).map_err(|_| OidcError::InvalidAuthorization)?;
        let metadata = CoreProviderMetadata::discover_async(issuer, &http)
            .await
            .map_err(|_| OidcError::Unavailable)?;
        let token_uri = metadata
            .token_endpoint()
            .cloned()
            .ok_or(OidcError::InvalidAuthorization)?;
        let client = CoreClient::from_provider_metadata(
            metadata,
            ClientId::new(purpose.client_id().into()),
            Some(ClientSecret::new(secret)),
        )
        .set_redirect_uri(
            RedirectUrl::new(format!("{origin}{}", purpose.callback()))
                .map_err(|_| OidcError::InvalidAuthorization)?,
        )
        .set_token_uri(token_uri);
        Ok(Self {
            client,
            http,
            issuer: public.to_owned(),
            purpose,
        })
    }

    pub fn login(&self) -> Login {
        let (challenge, verifier) = PkceCodeChallenge::new_random_sha256();
        let request = self
            .client
            .authorize_url(
                AuthenticationFlow::<CoreResponseType>::AuthorizationCode,
                CsrfToken::new_random,
                Nonce::new_random,
            )
            .add_scope(Scope::new("profile".into()))
            .set_pkce_challenge(challenge);
        let request = match self.purpose {
            // A fresh login lets the visitor choose another user even if a
            // Keycloak SSO cookie remains after local application logout.
            Purpose::AppLogin => request.add_extra_param("prompt", "login"),
            Purpose::Catalog => request
                .add_scope(Scope::new("selections:read".into()))
                .add_scope(Scope::new("offline_access".into()))
                .add_extra_param("prompt", "consent"),
        };
        let (url, state, nonce) = request.url();
        Login {
            url: url.to_string(),
            state,
            nonce,
            verifier,
        }
    }

    fn verify_identity(
        &self,
        response: &CoreTokenResponse,
        nonce: &Nonce,
        refreshing: bool,
    ) -> Result<Identity, OidcError> {
        let token = response
            .extra_fields()
            .id_token()
            .ok_or(OidcError::InvalidAuthorization)?;
        let verifier = self.client.id_token_verifier();
        let claims = token
            .claims(&verifier, |actual: Option<&Nonce>| {
                if refreshing && actual.is_none() {
                    Ok(())
                } else {
                    nonce.verify(actual)
                }
            })
            .map_err(|_| OidcError::InvalidAuthorization)?;
        if let Some(expected) = claims.access_token_hash() {
            let actual = AccessTokenHash::from_token(
                response.access_token(),
                token
                    .signing_alg()
                    .map_err(|_| OidcError::InvalidAuthorization)?,
                token
                    .signing_key(&verifier)
                    .map_err(|_| OidcError::InvalidAuthorization)?,
            )
            .map_err(|_| OidcError::InvalidAuthorization)?;
            if &actual != expected {
                return Err(OidcError::InvalidAuthorization);
            }
        }
        let subject = claims.subject().as_str().to_owned();
        if subject.is_empty() {
            return Err(OidcError::InvalidAuthorization);
        }
        let name = claims
            .preferred_username()
            .map(|n| n.as_str())
            .or_else(|| claims.name().and_then(|n| n.get(None)).map(|n| n.as_str()))
            .unwrap_or("Utilisateur")
            .to_owned();
        Ok(Identity {
            issuer: self.issuer.clone(),
            subject,
            name,
        })
    }

    fn tokens(
        response: &CoreTokenResponse,
        previous: Option<&RefreshToken>,
    ) -> Result<Tokens, OidcError> {
        if let Some(scopes) = response.scopes() {
            if !scopes.iter().any(|s| s.as_str() == "offline_access")
                || !scopes.iter().any(|s| s.as_str() == "selections:read")
            {
                return Err(OidcError::InvalidAuthorization);
            }
        } else if previous.is_none() {
            return Err(OidcError::InvalidAuthorization);
        }
        let offline = response
            .refresh_token()
            .or(previous)
            .cloned()
            .ok_or(OidcError::InvalidAuthorization)?;
        Ok(Tokens {
            access: response.access_token().clone(),
            offline,
            lifetime: response
                .expires_in()
                .ok_or(OidcError::InvalidAuthorization)?,
        })
    }

    pub async fn exchange(
        &self,
        code: String,
        verifier: PkceCodeVerifier,
        nonce: &Nonce,
    ) -> Result<(Identity, Tokens), OidcError> {
        if self.purpose != Purpose::Catalog {
            return Err(OidcError::InvalidAuthorization);
        }
        let response = self
            .client
            .exchange_code(AuthorizationCode::new(code))
            .set_pkce_verifier(verifier)
            .request_async(&self.http)
            .await
            .map_err(|_| OidcError::InvalidAuthorization)?;
        Ok((
            self.verify_identity(&response, nonce, false)?,
            Self::tokens(&response, None)?,
        ))
    }

    // Login tokens are validated then dropped. They never become catalogue
    // credentials and are not retained as part of the local session.
    pub async fn authenticate(
        &self,
        code: String,
        verifier: PkceCodeVerifier,
        nonce: &Nonce,
    ) -> Result<Identity, OidcError> {
        if self.purpose != Purpose::AppLogin {
            return Err(OidcError::InvalidAuthorization);
        }
        let response = self
            .client
            .exchange_code(AuthorizationCode::new(code))
            .set_pkce_verifier(verifier)
            .request_async(&self.http)
            .await
            .map_err(|_| OidcError::InvalidAuthorization)?;
        if response.scopes().is_some_and(|scopes| {
            scopes.iter().any(|s| {
                matches!(
                    s.as_str(),
                    "offline_access"
                        | "selections:read"
                        | "selections:write"
                        | "catalog:read"
                        | "catalog:write"
                        | "catalog:seed"
                )
            })
        }) {
            return Err(OidcError::InvalidAuthorization);
        }
        self.verify_identity(&response, nonce, false)
    }

    pub async fn refresh(
        &self,
        previous: &RefreshToken,
        nonce: &Nonce,
        subject: &str,
    ) -> Result<Tokens, OidcError> {
        if self.purpose != Purpose::Catalog {
            return Err(OidcError::InvalidAuthorization);
        }
        let response = self
            .client
            .exchange_refresh_token(previous)
            .request_async(&self.http)
            .await
            .map_err(|error| match error {
                RequestTokenError::ServerResponse(e)
                    if e.error() == &CoreErrorResponseType::InvalidGrant =>
                {
                    OidcError::Revoked
                }
                _ => OidcError::Unavailable,
            })?;
        if response.extra_fields().id_token().is_some()
            && self.verify_identity(&response, nonce, true)?.subject != subject
        {
            return Err(OidcError::InvalidAuthorization);
        }
        Self::tokens(&response, Some(previous))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn response(extra: serde_json::Value) -> CoreTokenResponse {
        let mut value = serde_json::json!({ "access_token": "test-access", "token_type": "Bearer", "expires_in": 60 });
        value
            .as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        serde_json::from_value(value).unwrap()
    }
    #[test]
    fn initial_authorization_requires_granted_offline_and_read_scopes_and_refresh_token() {
        for extra in [
            serde_json::json!({}),
            serde_json::json!({"refresh_token":"offline"}),
            serde_json::json!({"scope":"offline_access selections:read"}),
            serde_json::json!({"scope":"selections:read","refresh_token":"ordinary"}),
        ] {
            assert!(Oidc::tokens(&response(extra), None).is_err());
        }
        assert!(Oidc::tokens(&response(serde_json::json!({"scope":"offline_access selections:read","refresh_token":"offline"})), None).is_ok());
    }
    #[test]
    fn refresh_retains_or_rotates_the_offline_token_without_exposing_it() {
        let previous = RefreshToken::new("old".into());
        assert_eq!(
            Oidc::tokens(&response(serde_json::json!({})), Some(&previous))
                .unwrap()
                .offline
                .secret(),
            "old"
        );
        assert_eq!(
            Oidc::tokens(
                &response(serde_json::json!({"refresh_token":"next"})),
                Some(&previous)
            )
            .unwrap()
            .offline
            .secret(),
            "next"
        );
    }
    #[tokio::test]
    async fn backchannel_rejects_unexpected_hosts_before_network_access() {
        let transport = Backchannel {
            client: reqwest::Client::new(),
            public: "http://localhost:8081/realms/olaninc".into(),
            internal: "http://keycloak:8080/realms/olaninc".into(),
        };
        let mut request = HttpRequest::new(Vec::new());
        *request.uri_mut() = "http://untrusted.invalid/token".parse().unwrap();
        assert!(transport.call(request).await.is_err());
    }
}

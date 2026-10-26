//! Public-key retrieval and caching, separate from JWT and HTTP policy.
use jsonwebtoken::jwk::{Jwk, JwkSet};
use std::{
    error::Error,
    time::{Duration, Instant},
};
use tokio::sync::Mutex;

const CACHE_TTL: Duration = Duration::from_secs(300);
// Unknown kids cannot trigger an unbounded stream of requests to Keycloak.
const REFRESH_COOLDOWN: Duration = Duration::from_secs(30);

// This module only retrieves public keys. JWT policy and HTTP responses belong to its caller.
pub(super) struct JwksClient {
    url: String,
    client: reqwest::Client,
    cache: Mutex<KeyCache>,
}

#[derive(Default)]
struct KeyCache {
    keys: Option<JwkSet>,
    fetched_at: Option<Instant>,
    attempted_at: Option<Instant>,
}

#[derive(Debug)]
pub(super) enum JwksError {
    KeyNotFound,
    Unavailable,
}

impl JwksClient {
    pub(super) fn new(url: String) -> Result<Self, Box<dyn Error>> {
        reqwest::Url::parse(&url)?;
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .redirect(reqwest::redirect::Policy::none())
            .build()?;
        Ok(Self {
            url,
            client,
            cache: Mutex::new(KeyCache::default()),
        })
    }

    pub(super) async fn warm_up(&self) -> Result<(), JwksError> {
        let mut cache = self.cache.lock().await;
        self.refresh(&mut cache).await
    }

    async fn refresh(&self, cache: &mut KeyCache) -> Result<(), JwksError> {
        cache.attempted_at = Some(Instant::now());
        let result = async {
            let keys = self
                .client
                .get(&self.url)
                .send()
                .await?
                .error_for_status()?
                .json::<JwkSet>()
                .await?;
            Ok::<_, reqwest::Error>(keys)
        }
        .await;
        match result {
            Ok(keys) if !keys.keys.is_empty() => {
                cache.keys = Some(keys);
                cache.fetched_at = Some(Instant::now());
                Ok(())
            }
            _ => {
                tracing::warn!("Impossible de récupérer les clés publiques Keycloak");
                Err(JwksError::Unavailable)
            }
        }
    }

    pub(super) async fn key(&self, kid: &str) -> Result<Jwk, JwksError> {
        let mut cache = self.cache.lock().await;
        let fresh = cache.fetched_at.is_some_and(|at| at.elapsed() < CACHE_TTL);
        let known = cache
            .keys
            .as_ref()
            .and_then(|keys| keys.find(kid))
            .is_some();
        if !fresh || !known {
            let can_refresh = cache
                .attempted_at
                .is_none_or(|at| at.elapsed() >= REFRESH_COOLDOWN);
            if can_refresh {
                self.refresh(&mut cache).await?;
            } else if !fresh {
                return Err(JwksError::Unavailable);
            }
        }
        cache
            .keys
            .as_ref()
            .and_then(|keys| keys.find(kid))
            .cloned()
            .ok_or(JwksError::KeyNotFound)
    }
}

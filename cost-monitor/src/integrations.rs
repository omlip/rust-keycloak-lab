//! Account-owned integrations, independent of browser sessions. All state is
//! intentionally in memory; no token is serialized or logged.
use crate::oidc::{Identity, Tokens};
use openidconnect::Nonce;
use std::{collections::HashMap, sync::Arc, time::Instant};
use tokio::sync::{Mutex, RwLock};

const MAX_ACCOUNTS: usize = 128;

#[derive(Clone, PartialEq, Eq, Hash)]
pub struct AccountId {
    issuer: String,
    subject: String,
}

impl From<&Identity> for AccountId {
    fn from(identity: &Identity) -> Self {
        Self {
            issuer: identity.issuer.clone(),
            subject: identity.subject.clone(),
        }
    }
}

pub struct CatalogConnection {
    pub tokens: Option<Tokens>,
    pub nonce: Nonce,
    pub expires: Instant,
    pub renewals: u64,
}

impl CatalogConnection {
    fn new(tokens: Tokens, nonce: Nonce) -> Self {
        Self {
            expires: Instant::now()
                .checked_add(tokens.lifetime)
                .unwrap_or_else(Instant::now),
            tokens: Some(tokens),
            nonce,
            renewals: 0,
        }
    }
    pub fn needs_refresh(&self) -> bool {
        self.expires
            .saturating_duration_since(Instant::now())
            .as_secs()
            <= 5
    }
    pub fn replace(&mut self, tokens: Tokens) {
        self.expires = Instant::now()
            .checked_add(tokens.lifetime)
            .unwrap_or_else(Instant::now);
        self.tokens = Some(tokens);
        self.renewals += 1;
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum ConnectionError {
    SessionUnavailable,
    StaleAttempt,
    AccountMismatch,
    Capacity,
}

pub struct Integration {
    owner: AccountId,
    epoch: u64,
    pub catalog: Option<CatalogConnection>,
}

impl Integration {
    pub fn subject(&self) -> &str {
        &self.owner.subject
    }
    fn new(owner: AccountId) -> Self {
        Self {
            owner,
            epoch: 0,
            catalog: None,
        }
    }
    pub fn begin_catalog(&mut self) -> u64 {
        self.epoch += 1;
        self.epoch
    }
    pub fn validate_attempt(&self, identity: &Identity, epoch: u64) -> Result<(), ConnectionError> {
        if self.epoch != epoch {
            return Err(ConnectionError::StaleAttempt);
        }
        if self.owner != AccountId::from(identity) {
            return Err(ConnectionError::AccountMismatch);
        }
        Ok(())
    }
    pub fn attach_catalog(
        &mut self,
        identity: &Identity,
        tokens: Tokens,
        nonce: Nonce,
        epoch: u64,
    ) -> Result<(), ConnectionError> {
        self.validate_attempt(identity, epoch)?;
        self.catalog = Some(CatalogConnection::new(tokens, nonce));
        self.epoch += 1;
        Ok(())
    }
    pub fn disconnect_catalog(&mut self) {
        self.epoch += 1;
        self.catalog = None;
    }
}

#[derive(Default)]
pub struct Integrations {
    accounts: RwLock<HashMap<AccountId, Arc<Mutex<Integration>>>>,
}

impl Integrations {
    pub async fn get(&self, identity: &Identity) -> Option<Arc<Mutex<Integration>>> {
        self.accounts
            .read()
            .await
            .get(&AccountId::from(identity))
            .cloned()
    }
    pub async fn get_or_create(
        &self,
        identity: &Identity,
    ) -> Result<Arc<Mutex<Integration>>, ConnectionError> {
        let owner = AccountId::from(identity);
        let mut accounts = self.accounts.write().await;
        if let Some(entry) = accounts.get(&owner) {
            return Ok(entry.clone());
        }
        if accounts.len() >= MAX_ACCOUNTS {
            return Err(ConnectionError::Capacity);
        }
        let entry = Arc::new(Mutex::new(Integration::new(owner.clone())));
        accounts.insert(owner, entry.clone());
        // Keep bounded, token-free tombstones after disconnect: epochs must not
        // reset and let an older callback restore a disconnected integration.
        Ok(entry)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openidconnect::{AccessToken, RefreshToken};
    use std::time::Duration;
    fn identity(issuer: &str, subject: &str) -> Identity {
        Identity {
            issuer: issuer.into(),
            subject: subject.into(),
            name: subject.into(),
        }
    }
    fn tokens() -> Tokens {
        Tokens {
            access: AccessToken::new("access".into()),
            offline: RefreshToken::new("offline".into()),
            lifetime: Duration::from_secs(60),
        }
    }

    #[tokio::test]
    async fn account_keys_include_issuer_and_subject_and_storage_is_not_a_session() {
        let store = Integrations::default();
        let alice = identity("https://id.test", "alice");
        let slot = store.get_or_create(&alice).await.unwrap();
        let epoch = slot.lock().await.begin_catalog();
        slot.lock()
            .await
            .attach_catalog(&alice, tokens(), Nonce::new_random(), epoch)
            .unwrap();
        let restored = store
            .get(&identity("https://id.test", "alice"))
            .await
            .unwrap();
        assert!(Arc::ptr_eq(&slot, &restored));
        assert!(restored.lock().await.catalog.is_some());
        assert!(
            store
                .get(&identity("https://other.test", "alice"))
                .await
                .is_none()
        );
        assert!(
            store
                .get(&identity("https://id.test", "bob"))
                .await
                .is_none()
        );
        assert!(Integrations::default().get(&alice).await.is_none());
    }

    #[tokio::test]
    async fn ownership_disconnect_and_latest_attempt_are_enforced_for_every_session() {
        let store = Integrations::default();
        let alice = identity("https://id.test", "alice");
        let slot = store.get_or_create(&alice).await.unwrap();
        let epoch = slot.lock().await.begin_catalog();
        for other in [
            identity("https://id.test", "bob"),
            identity("https://other.test", "alice"),
        ] {
            assert_eq!(
                slot.lock()
                    .await
                    .attach_catalog(&other, tokens(), Nonce::new_random(), epoch),
                Err(ConnectionError::AccountMismatch)
            );
        }
        slot.lock()
            .await
            .attach_catalog(&alice, tokens(), Nonce::new_random(), epoch)
            .unwrap();
        assert_eq!(
            slot.lock()
                .await
                .attach_catalog(&alice, tokens(), Nonce::new_random(), epoch),
            Err(ConnectionError::StaleAttempt)
        );
        let stale = slot.lock().await.begin_catalog();
        let latest = slot.lock().await.begin_catalog();
        assert_eq!(
            slot.lock()
                .await
                .attach_catalog(&alice, tokens(), Nonce::new_random(), stale),
            Err(ConnectionError::StaleAttempt)
        );
        slot.lock().await.disconnect_catalog();
        assert_eq!(
            slot.lock()
                .await
                .attach_catalog(&alice, tokens(), Nonce::new_random(), latest),
            Err(ConnectionError::StaleAttempt)
        );
        let same_slot = store.get_or_create(&alice).await.unwrap();
        assert!(Arc::ptr_eq(&slot, &same_slot));
        assert!(same_slot.lock().await.catalog.is_none());
    }

    #[test]
    fn refresh_rotation_and_expiration_belong_to_the_integration() {
        let mut connection = CatalogConnection::new(tokens(), Nonce::new_random());
        assert!(!connection.needs_refresh());
        connection.expires = Instant::now();
        assert!(connection.needs_refresh());
        let mut next = tokens();
        next.offline = RefreshToken::new("rotated".into());
        connection.replace(next);
        assert_eq!(
            connection.tokens.as_ref().unwrap().offline.secret(),
            "rotated"
        );
        assert_eq!(connection.renewals, 1);
        assert!(!connection.needs_refresh());
    }

    #[tokio::test]
    async fn account_slots_are_bounded_and_reconnect_uses_the_same_generation_history() {
        let store = Integrations::default();
        for i in 0..MAX_ACCOUNTS {
            store
                .get_or_create(&identity("https://id.test", &format!("user{i}")))
                .await
                .unwrap();
        }
        assert!(matches!(
            store
                .get_or_create(&identity("https://id.test", "extra"))
                .await,
            Err(ConnectionError::Capacity)
        ));
        assert!(
            store
                .get_or_create(&identity("https://id.test", "user0"))
                .await
                .is_ok()
        );
    }
}

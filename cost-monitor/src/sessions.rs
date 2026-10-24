use crate::{
    integrations::ConnectionError,
    oidc::{Identity, Login, Purpose},
};
use openidconnect::CsrfToken;
use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::{Mutex, RwLock};

pub const SESSION_TTL: Duration = Duration::from_secs(86400);
pub const LOGIN_TTL: Duration = Duration::from_secs(300);
pub const LIMIT: usize = 128;

pub struct Pending {
    pub login: Login,
    pub created: Instant,
    pub purpose: Purpose,
    pub binding: Option<CatalogBinding>,
}

pub struct CatalogBinding {
    pub session_id: String,
    // Generation of the account integration, not of the browser session.
    pub epoch: u64,
}

pub struct Session {
    pub identity: Identity,
    pub created: Instant,
    pub csrf: String,
    closed: bool,
}

impl Session {
    pub fn new(identity: Identity) -> Self {
        Self {
            identity,
            created: Instant::now(),
            csrf: random(),
            closed: false,
        }
    }
    pub fn expired(&self) -> bool {
        self.closed || self.created.elapsed() >= SESSION_TTL
    }
    pub fn ensure_account(&self, identity: &Identity) -> Result<(), ConnectionError> {
        if self.expired() {
            return Err(ConnectionError::SessionUnavailable);
        }
        if !self.identity.same_account(identity) {
            return Err(ConnectionError::AccountMismatch);
        }
        Ok(())
    }
    pub fn close(&mut self) {
        self.closed = true;
    }
}

#[derive(Default)]
pub struct Sessions {
    pub pending: Mutex<HashMap<String, Pending>>,
    pub active: RwLock<HashMap<String, Arc<Mutex<Session>>>>,
}

impl Sessions {
    pub async fn get(&self, id: &str) -> Option<Arc<Mutex<Session>>> {
        let session = self.active.read().await.get(id).cloned()?;
        let mut value = session.lock().await;
        if value.expired() {
            value.close();
            drop(value);
            self.active.write().await.remove(id);
            return None;
        }
        drop(value);
        Some(session)
    }

    pub async fn insert(&self, session: Session) -> Option<String> {
        // Snapshot arcs first: never hold the global map while acquiring a
        // session lock or doing network work.
        let entries: Vec<_> = self
            .active
            .read()
            .await
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        for (id, session) in entries {
            let mut value = session.lock().await;
            if value.expired() {
                value.close();
                drop(value);
                self.active.write().await.remove(&id);
            }
        }
        let mut active = self.active.write().await;
        if active.len() >= LIMIT {
            return None;
        }
        let id = random();
        active.insert(id.clone(), Arc::new(Mutex::new(session)));
        Some(id)
    }

    pub async fn cancel_pending(&self, session_id: &str) {
        self.pending.lock().await.retain(|_, pending| {
            pending
                .binding
                .as_ref()
                .is_none_or(|b| b.session_id != session_id)
        });
    }
}

pub fn random() -> String {
    CsrfToken::new_random_len(32).secret().to_owned()
}

pub fn equal_secret(left: &str, right: &str) -> bool {
    use subtle::ConstantTimeEq;
    bool::from(left.as_bytes().ct_eq(right.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn session() -> Session {
        Session::new(Identity {
            issuer: "https://identity.test/realm".into(),
            subject: "alice".into(),
            name: "Alice".into(),
        })
    }
    #[test]
    fn a_session_only_authenticates_its_own_account_until_closed() {
        let mut session = session();
        for identity in [
            Identity {
                issuer: session.identity.issuer.clone(),
                subject: "bob".into(),
                name: "Bob".into(),
            },
            Identity {
                issuer: "https://other.test/realm".into(),
                subject: "alice".into(),
                name: "Alice".into(),
            },
        ] {
            assert_eq!(
                session.ensure_account(&identity),
                Err(ConnectionError::AccountMismatch)
            );
            assert_eq!(session.identity.subject, "alice");
        }
        let identity = session.identity.clone();
        assert!(session.ensure_account(&identity).is_ok());
        session.close();
        assert_eq!(
            session.ensure_account(&identity),
            Err(ConnectionError::SessionUnavailable)
        );
    }
    #[tokio::test]
    async fn sessions_are_random_bounded_and_expire_without_exposing_tokens() {
        let store = Sessions::default();
        let id = store.insert(session()).await.unwrap();
        assert!(id.len() >= 32);
        assert!(!id.contains("offline"));
        assert!(store.get("unknown").await.is_none());
        let saved = store.get(&id).await.unwrap();
        saved.lock().await.created = Instant::now() - SESSION_TTL;
        assert!(store.get(&id).await.is_none());
        for _ in 0..LIMIT {
            store.insert(session()).await.unwrap();
        }
        assert!(store.insert(session()).await.is_none());
    }
}

//! Bounded cancellation contexts for verified JWT sessions, NOT a login store.
//! Every API call must first validate its Bearer token. No cookie authenticates.
use crate::{
    auth::Principal,
    integrations::ConnectionError,
    oidc::{Identity, Login},
};
use openidconnect::CsrfToken;
use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tokio::sync::{Mutex, RwLock};

pub const LOGIN_TTL: Duration = Duration::from_secs(300);
pub const LIMIT: usize = 128;
pub struct Pending {
    pub login: Login,
    pub created: Instant,
    pub binding: CatalogBinding,
    pub code: Option<String>,
}
pub struct CatalogBinding {
    pub session_id: String,
    pub session: Arc<Mutex<Session>>,
    pub epoch: u64,
}
pub struct Session {
    pub identity: Identity,
    pub expires: Instant,
    closed: bool,
}
impl Session {
    pub fn expired(&self) -> bool {
        self.closed || Instant::now() >= self.expires
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
    active: RwLock<HashMap<String, Arc<Mutex<Session>>>>,
}
impl Sessions {
    pub async fn bind(
        &self,
        principal: &Principal,
    ) -> Result<Arc<Mutex<Session>>, ConnectionError> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let remaining = principal
            .exp
            .checked_sub(now)
            .filter(|n| *n > 0)
            .ok_or(ConnectionError::SessionUnavailable)?;
        let entries: Vec<_> = self
            .active
            .read()
            .await
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        for (sid, context) in entries {
            let mut context = context.lock().await;
            if Instant::now() >= context.expires {
                context.close();
                self.active.write().await.remove(&sid);
            }
        }
        // Do not hold the map while waiting for a context used by an API call.
        let context = {
            let mut active = self.active.write().await;
            if let Some(context) = active.get(&principal.sid) {
                context.clone()
            } else {
                if active.len() >= LIMIT {
                    return Err(ConnectionError::Capacity);
                }
                let context = Arc::new(Mutex::new(Session {
                    identity: principal.identity(),
                    expires: Instant::now() + Duration::from_secs(remaining),
                    closed: false,
                }));
                active.insert(principal.sid.clone(), context.clone());
                context
            }
        };
        {
            let mut value = context.lock().await;
            value.ensure_account(&principal.identity())?;
            value.expires = Instant::now() + Duration::from_secs(remaining);
            value.identity = principal.identity();
        }
        Ok(context)
    }
    pub async fn cancel_pending(&self, sid: &str) {
        self.pending
            .lock()
            .await
            .retain(|_, pending| pending.binding.session_id != sid);
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
    #[tokio::test]
    async fn contexts_are_bound_to_verified_sid_identity_expiry_and_cancellation() {
        let store = Sessions::default();
        let principal: Principal =
            serde_json::from_value(crate::test_issuer::claims("http://issuer", "alice", "sid"))
                .unwrap();
        let context = store.bind(&principal).await.unwrap();
        assert!(Arc::ptr_eq(
            &context,
            &store.bind(&principal).await.unwrap()
        ));
        let mut other = principal.clone();
        other.sub = "bob".into();
        assert!(matches!(
            store.bind(&other).await,
            Err(ConnectionError::AccountMismatch)
        ));
        context.lock().await.close();
        assert!(matches!(
            store.bind(&principal).await,
            Err(ConnectionError::SessionUnavailable)
        ));
        assert!(
            context
                .lock()
                .await
                .ensure_account(&principal.identity())
                .is_err()
        );
        other.sid = "new-sid".into();
        assert!(store.bind(&other).await.is_ok());
        other.exp = 1;
        assert!(store.bind(&other).await.is_err());
        for n in 2..LIMIT {
            other.exp = principal.exp;
            other.sid = n.to_string();
            store.bind(&other).await.unwrap();
        }
        other.sid = "over-capacity".into();
        assert!(matches!(
            store.bind(&other).await,
            Err(ConnectionError::Capacity)
        ));
    }
}

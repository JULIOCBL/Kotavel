use super::{SQLiteSessionStore, StoreError};
use serde_json::Value;
use std::{sync::Arc, time::Duration};

/// Opaque identity for one durable Kotavel installation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionInstanceId {
    pub value: String,
}

/// Opaque namespace for data intentionally shared between linked sessions.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionContextId {
    pub value: String,
}

/// Short lived bearer credential that can be redeemed by one HTTP session.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionLinkGrant {
    pub value: String,
}

/// Durable port for installation, shared-context, and authorized session-link
/// operations. Implementations must redeem each grant atomically and only once.
pub trait SessionContextStore: Send + Sync {
    fn instance_id(&self) -> SessionInstanceId;
    fn create_context(&self) -> Result<SessionContextId, StoreError>;
    fn default_context(&self) -> Result<SessionContextId, StoreError>;
    fn delete_context(&self, context: &SessionContextId) -> Result<bool, StoreError>;
    fn issue_link_grant(
        &self,
        context: &SessionContextId,
        lifetime: Duration,
    ) -> Result<SessionLinkGrant, StoreError>;
    fn redeem_link_grant(
        &self,
        grant: &SessionLinkGrant,
        session_id: &str,
    ) -> Result<bool, StoreError>;
    fn context_for_session(&self, session_id: &str)
    -> Result<Option<SessionContextId>, StoreError>;
    fn revoke_context_link(&self, session_id: &str) -> Result<bool, StoreError>;
    fn put_context_value(
        &self,
        context: &SessionContextId,
        key: &str,
        value: Value,
        lifetime: Option<Duration>,
    ) -> Result<(), StoreError>;
    fn put_context_value_for_session(
        &self,
        session_id: &str,
        key: &str,
        value: Value,
        lifetime: Option<Duration>,
    ) -> Result<bool, StoreError>;
    fn get_context_value(
        &self,
        context: &SessionContextId,
        key: &str,
    ) -> Result<Option<Value>, StoreError>;
    fn get_context_value_for_session(
        &self,
        session_id: &str,
        key: &str,
    ) -> Result<Option<Value>, StoreError>;
}

impl SessionContextStore for SQLiteSessionStore {
    fn instance_id(&self) -> SessionInstanceId {
        SQLiteSessionStore::instance_id(self)
    }

    fn create_context(&self) -> Result<SessionContextId, StoreError> {
        SQLiteSessionStore::create_context(self)
    }

    fn default_context(&self) -> Result<SessionContextId, StoreError> {
        SQLiteSessionStore::default_context(self)
    }

    fn delete_context(&self, context: &SessionContextId) -> Result<bool, StoreError> {
        SQLiteSessionStore::delete_context(self, context)
    }

    fn issue_link_grant(
        &self,
        context: &SessionContextId,
        lifetime: Duration,
    ) -> Result<SessionLinkGrant, StoreError> {
        SQLiteSessionStore::issue_link_grant(self, context, lifetime)
    }

    fn redeem_link_grant(
        &self,
        grant: &SessionLinkGrant,
        session_id: &str,
    ) -> Result<bool, StoreError> {
        SQLiteSessionStore::redeem_link_grant(self, grant, session_id)
    }

    fn context_for_session(
        &self,
        session_id: &str,
    ) -> Result<Option<SessionContextId>, StoreError> {
        SQLiteSessionStore::context_for_session(self, session_id)
    }

    fn revoke_context_link(&self, session_id: &str) -> Result<bool, StoreError> {
        SQLiteSessionStore::revoke_context_link(self, session_id)
    }

    fn put_context_value(
        &self,
        context: &SessionContextId,
        key: &str,
        value: Value,
        lifetime: Option<Duration>,
    ) -> Result<(), StoreError> {
        SQLiteSessionStore::put_context_value(self, context, key, value, lifetime)
    }

    fn put_context_value_for_session(
        &self,
        session_id: &str,
        key: &str,
        value: Value,
        lifetime: Option<Duration>,
    ) -> Result<bool, StoreError> {
        SQLiteSessionStore::put_context_value_for_session(self, session_id, key, value, lifetime)
    }

    fn get_context_value(
        &self,
        context: &SessionContextId,
        key: &str,
    ) -> Result<Option<Value>, StoreError> {
        SQLiteSessionStore::get_context_value(self, context, key)
    }

    fn get_context_value_for_session(
        &self,
        session_id: &str,
        key: &str,
    ) -> Result<Option<Value>, StoreError> {
        SQLiteSessionStore::get_context_value_for_session(self, session_id, key)
    }
}

/// Explicit bridge between trusted application state and an authenticated session.
#[derive(Clone)]
pub struct SessionContextResolver {
    store: Arc<dyn SessionContextStore>,
}

impl SessionContextResolver {
    /// Create a resolver backed by SQLite, preserving the initial API.
    pub fn new(store: Arc<SQLiteSessionStore>) -> Self {
        Self::with_store(store)
    }

    /// Create a resolver backed by any implementation of the context store port.
    pub fn with_store(store: Arc<dyn SessionContextStore>) -> Self {
        Self { store }
    }

    pub fn instance_id(&self) -> SessionInstanceId {
        self.store.instance_id()
    }

    pub fn create_context(&self) -> Result<SessionContextId, StoreError> {
        self.store.create_context()
    }

    pub fn default_context(&self) -> Result<SessionContextId, StoreError> {
        self.store.default_context()
    }

    pub fn delete_context(&self, context: &SessionContextId) -> Result<bool, StoreError> {
        self.store.delete_context(context)
    }

    pub fn issue_link_grant(
        &self,
        context: &SessionContextId,
    ) -> Result<SessionLinkGrant, StoreError> {
        self.issue_link_grant_for(context, Duration::from_secs(60))
    }

    pub fn issue_link_grant_for(
        &self,
        context: &SessionContextId,
        lifetime: Duration,
    ) -> Result<SessionLinkGrant, StoreError> {
        self.store.issue_link_grant(context, lifetime)
    }

    pub fn redeem_link_grant(
        &self,
        grant: &SessionLinkGrant,
        session_id: &str,
    ) -> Result<bool, StoreError> {
        self.store.redeem_link_grant(grant, session_id)
    }

    pub fn context_for_session(
        &self,
        session_id: &str,
    ) -> Result<Option<SessionContextId>, StoreError> {
        self.store.context_for_session(session_id)
    }

    pub fn linked_session(
        self: &Arc<Self>,
        session_id: &str,
    ) -> Result<Option<LinkedSessionContext>, StoreError> {
        if self.context_for_session(session_id)?.is_none() {
            return Ok(None);
        }
        Ok(Some(LinkedSessionContext {
            resolver: Arc::clone(self),
            session_id: session_id.to_owned(),
        }))
    }

    pub fn revoke_context_link(&self, session_id: &str) -> Result<bool, StoreError> {
        self.store.revoke_context_link(session_id)
    }

    pub fn put(
        &self,
        context: &SessionContextId,
        key: &str,
        value: Value,
        lifetime: Option<Duration>,
    ) -> Result<(), StoreError> {
        self.store.put_context_value(context, key, value, lifetime)
    }

    pub fn get(&self, context: &SessionContextId, key: &str) -> Result<Option<Value>, StoreError> {
        self.store.get_context_value(context, key)
    }

    pub fn get_for_session(
        &self,
        session_id: &str,
        key: &str,
    ) -> Result<Option<Value>, StoreError> {
        self.store.get_context_value_for_session(session_id, key)
    }

    pub fn put_for_session(
        &self,
        session_id: &str,
        key: &str,
        value: Value,
        lifetime: Option<Duration>,
    ) -> Result<bool, StoreError> {
        self.store
            .put_context_value_for_session(session_id, key, value, lifetime)
    }
}

/// Request-scoped access to the shared data authorized for one HTTP session.
/// Each operation rechecks the live link so revocation takes effect immediately.
#[derive(Clone)]
pub struct LinkedSessionContext {
    resolver: Arc<SessionContextResolver>,
    session_id: String,
}

impl LinkedSessionContext {
    pub fn get(&self, key: &str) -> Result<Option<Value>, StoreError> {
        self.resolver.get_for_session(&self.session_id, key)
    }

    /// Returns false if the context link was revoked or the session expired.
    pub fn put(
        &self,
        key: &str,
        value: Value,
        lifetime: Option<Duration>,
    ) -> Result<bool, StoreError> {
        self.resolver
            .put_for_session(&self.session_id, key, value, lifetime)
    }

    pub fn revoke(&self) -> Result<bool, StoreError> {
        self.resolver.revoke_context_link(&self.session_id)
    }
}

/// Owner-side handle to durable data shared only after a grant is redeemed.
pub struct SessionContext {
    resolver: Arc<SessionContextResolver>,
    id: SessionContextId,
}

impl SessionContext {
    pub fn create(resolver: Arc<SessionContextResolver>) -> Result<Self, StoreError> {
        let id = resolver.create_context()?;
        Ok(Self { resolver, id })
    }

    pub fn default_for(resolver: Arc<SessionContextResolver>) -> Result<Self, StoreError> {
        let id = resolver.default_context()?;
        Ok(Self { resolver, id })
    }

    pub fn id(&self) -> &SessionContextId {
        &self.id
    }

    pub fn issue_link_grant(&self) -> Result<SessionLinkGrant, StoreError> {
        self.resolver.issue_link_grant(&self.id)
    }

    pub fn issue_link_grant_for(&self, lifetime: Duration) -> Result<SessionLinkGrant, StoreError> {
        self.resolver.issue_link_grant_for(&self.id, lifetime)
    }

    pub fn put(
        &self,
        key: &str,
        value: Value,
        lifetime: Option<Duration>,
    ) -> Result<(), StoreError> {
        self.resolver.put(&self.id, key, value, lifetime)
    }

    pub fn get(&self, key: &str) -> Result<Option<Value>, StoreError> {
        self.resolver.get(&self.id, key)
    }

    pub fn revoke(&self) -> Result<bool, StoreError> {
        self.resolver.delete_context(&self.id)
    }
}

#[cfg(test)]
#[path = "../../tests/unit/session/context.rs"]
mod tests;

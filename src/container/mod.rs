//! Thread-safe dependency-injection container.
//!
//! Registration, singleton initialization and shutdown are separated from
//! dependency resolution so their concurrency rules stay explicit.

mod error;
mod provider;
mod resolution;
mod state;

#[cfg(test)]
mod tests;

pub use error::ContainerError;
pub use provider::{
    CreateServiceProviders, ProviderError, ProviderLifecycle, ProviderLifecycleError,
    ServiceProvider, ServiceProviderList,
};

use state::{Factory, Lifetime, Registration, State, ensure_registrable};
use std::{
    any::{Any, TypeId, type_name},
    sync::{Arc, Condvar, Mutex, atomic::Ordering},
};

/// Factories may be registered during startup; services can then be resolved
/// safely from HTTP, background and other runtime worker threads.
pub struct Container {
    id: u64,
    state: Mutex<State>,
    changed: Condvar,
}

impl Default for Container {
    fn default() -> Self {
        Self {
            id: state::NEXT_CONTAINER_ID.fetch_add(1, Ordering::Relaxed),
            state: Mutex::new(State::default()),
            changed: Condvar::new(),
        }
    }
}

impl Container {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn singleton<T, F>(&self, factory: F) -> Result<(), ContainerError>
    where
        T: Any + Send + Sync,
        F: Fn(&Container) -> Result<Arc<T>, ContainerError> + Send + Sync + 'static,
    {
        self.register::<T, F>(factory, Lifetime::Singleton)
    }

    pub fn bind<T, F>(&self, factory: F) -> Result<(), ContainerError>
    where
        T: Any + Send + Sync,
        F: Fn(&Container) -> Result<Arc<T>, ContainerError> + Send + Sync + 'static,
    {
        self.register::<T, F>(factory, Lifetime::Transient)
    }

    pub fn instance<T: Any + Send + Sync>(&self, value: Arc<T>) -> Result<(), ContainerError> {
        let mut state = self.lock();
        ensure_registrable(&state)?;
        let id = TypeId::of::<T>();
        if state.registrations.contains_key(&id) {
            return Err(ContainerError::Duplicate(type_name::<T>()));
        }
        state.construction_order.push(id);
        state.registrations.insert(
            id,
            Registration {
                name: type_name::<T>(),
                lifetime: Lifetime::Singleton,
                factory: Arc::new(|_| {
                    Err(ContainerError::FactoryFailed {
                        service: type_name::<T>(),
                        reason: "instance has no factory".into(),
                    })
                }),
                value: Some(value),
            },
        );
        Ok(())
    }

    pub fn get<T: Any + Send + Sync>(&self) -> Result<Arc<T>, ContainerError> {
        resolution::resolve::<T>(self, Lifetime::Singleton)
    }
    pub fn make<T: Any + Send + Sync>(&self) -> Result<Arc<T>, ContainerError> {
        resolution::resolve::<T>(self, Lifetime::Transient)
    }
    pub fn contains<T: Any>(&self) -> bool {
        self.lock().registrations.contains_key(&TypeId::of::<T>())
    }

    pub fn seal(&self) -> Result<(), ContainerError> {
        let mut state = self.lock();
        if state.stopped {
            return Err(ContainerError::Stopped);
        }
        state.sealed = true;
        Ok(())
    }

    /// Drops container-owned instances in reverse construction order.
    pub fn shutdown(&self) {
        let values = {
            let mut state = self.lock();
            if state.stopped {
                return;
            }
            state.stopped = true;
            let mut values = Vec::new();
            for id in std::mem::take(&mut state.construction_order)
                .into_iter()
                .rev()
            {
                if let Some(registration) = state.registrations.get_mut(&id) {
                    if let Some(value) = registration.value.take() {
                        values.push(value);
                    }
                }
            }
            for registration in state.registrations.values_mut() {
                if let Some(value) = registration.value.take() {
                    values.push(value);
                }
            }
            self.changed.notify_all();
            values
        };
        drop(values);
    }

    pub fn sealed(&self) -> bool {
        self.lock().sealed
    }
    pub fn stopped(&self) -> bool {
        self.lock().stopped
    }

    fn register<T, F>(&self, factory: F, lifetime: Lifetime) -> Result<(), ContainerError>
    where
        T: Any + Send + Sync,
        F: Fn(&Container) -> Result<Arc<T>, ContainerError> + Send + Sync + 'static,
    {
        let mut state = self.lock();
        ensure_registrable(&state)?;
        let id = TypeId::of::<T>();
        if state.registrations.contains_key(&id) {
            return Err(ContainerError::Duplicate(type_name::<T>()));
        }
        let factory: Factory =
            Arc::new(move |container| factory(container).map(|value| value as Arc<state::Erased>));
        state.registrations.insert(
            id,
            Registration {
                name: type_name::<T>(),
                lifetime,
                factory,
                value: None,
            },
        );
        Ok(())
    }

    pub(in crate::container) fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

impl Drop for Container {
    fn drop(&mut self) {
        self.shutdown();
    }
}

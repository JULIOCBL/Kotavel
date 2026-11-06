use super::Container;
use std::{
    error::Error,
    io,
    panic::{AssertUnwindSafe, catch_unwind},
};

pub type ProviderError = Box<dyn Error + Send + Sync + 'static>;
pub type ServiceProviderList = Vec<Box<dyn ServiceProvider>>;
/// Factory boundary used by runtimes to create application-owned providers.
pub type CreateServiceProviders = dyn Fn() -> ServiceProviderList + Sync;

/// Separates service registration from boot and shutdown hooks.
pub trait ServiceProvider: Send {
    fn register_services(&mut self, services: &Container) -> Result<(), ProviderError>;

    fn boot(&mut self, _services: &Container) -> Result<(), ProviderError> {
        Ok(())
    }

    /// Cleanup hooks are isolated so one faulty provider cannot skip later cleanup.
    fn shutdown(&mut self, _services: &Container) {}
}

/// Owns providers and unwinds successfully attempted boots in reverse order.
///
/// A provider whose `boot` returns an error is included in cleanup because it
/// may have acquired resources before reporting that error.
pub struct ProviderLifecycle<'a> {
    services: &'a Container,
    providers: ServiceProviderList,
    boot_attempted: usize,
    registration_started: bool,
    registered: bool,
    boot_started: bool,
    stopped: bool,
}

impl<'a> ProviderLifecycle<'a> {
    pub fn new(services: &'a Container, providers: ServiceProviderList) -> Self {
        Self {
            services,
            providers,
            boot_attempted: 0,
            registration_started: false,
            registered: false,
            boot_started: false,
            stopped: false,
        }
    }

    pub fn register_services(&mut self) -> Result<(), ProviderLifecycleError> {
        if self.registration_started || self.stopped {
            return Err(invalid_lifecycle(self.providers.len(), "register"));
        }
        self.registration_started = true;
        for index in 0..self.providers.len() {
            self.providers[index]
                .register_services(self.services)
                .map_err(|source| ProviderLifecycleError {
                    index,
                    phase: "register",
                    source,
                })?;
        }
        self.registered = true;
        Ok(())
    }

    pub fn boot(&mut self) -> Result<(), ProviderLifecycleError> {
        if !self.registered || self.boot_started || self.stopped {
            return Err(invalid_lifecycle(self.boot_attempted, "boot"));
        }
        self.boot_started = true;
        while self.boot_attempted < self.providers.len() {
            let index = self.boot_attempted;
            self.boot_attempted += 1;
            self.providers[index]
                .boot(self.services)
                .map_err(|source| ProviderLifecycleError {
                    index,
                    phase: "boot",
                    source,
                })?;
        }
        Ok(())
    }

    pub fn shutdown(&mut self) {
        if self.stopped {
            return;
        }
        self.stopped = true;
        while self.boot_attempted > 0 {
            self.boot_attempted -= 1;
            let provider = &mut self.providers[self.boot_attempted];
            let _ = catch_unwind(AssertUnwindSafe(|| provider.shutdown(self.services)));
        }
    }

    pub fn providers(&self) -> &[Box<dyn ServiceProvider>] {
        &self.providers
    }
}

impl Drop for ProviderLifecycle<'_> {
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[derive(Debug)]
pub struct ProviderLifecycleError {
    pub index: usize,
    pub phase: &'static str,
    source: ProviderError,
}

impl std::fmt::Display for ProviderLifecycleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "service provider {} failed during {}: {}",
            self.index, self.phase, self.source
        )
    }
}

impl Error for ProviderLifecycleError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(self.source.as_ref())
    }
}

fn invalid_lifecycle(index: usize, phase: &'static str) -> ProviderLifecycleError {
    ProviderLifecycleError {
        index,
        phase,
        source: Box::new(io::Error::other("invalid provider lifecycle transition")),
    }
}

#[cfg(test)]
#[path = "../../tests/unit/container/provider.rs"]
mod tests;

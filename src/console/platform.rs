use crate::{
    container::{Container, ProviderError, ServiceProvider},
    contracts::{
        Filesystem, NetworkAddressesProvider, PlatformServices, ProcessMetricsProvider,
        RuntimePathResolver, SystemPaths,
    },
    filesystem::LocalFilesystem,
    metrics::NativeProcessMetricsProvider,
    system::{NativeNetworkAddressesProvider, NativeRuntimePathResolver, NativeSystemPaths},
};
use std::sync::Arc;

pub(super) struct NativePlatformProvider;

impl ServiceProvider for NativePlatformProvider {
    fn register_services(&mut self, services: &Container) -> Result<(), ProviderError> {
        if !services.contains::<Arc<dyn Filesystem>>() {
            services
                .instance::<Arc<dyn Filesystem>>(Arc::new(Arc::new(LocalFilesystem)))
                .map_err(provider_error)?;
        }
        if !services.contains::<Arc<dyn ProcessMetricsProvider>>() {
            let provider: Arc<dyn ProcessMetricsProvider> = Arc::new(NativeProcessMetricsProvider);
            services
                .instance::<Arc<dyn ProcessMetricsProvider>>(Arc::new(provider))
                .map_err(provider_error)?;
        }
        if !services.contains::<Arc<dyn SystemPaths>>() {
            let provider: Arc<dyn SystemPaths> = Arc::new(NativeSystemPaths);
            services
                .instance::<Arc<dyn SystemPaths>>(Arc::new(provider))
                .map_err(provider_error)?;
        }
        if !services.contains::<Arc<dyn RuntimePathResolver>>() {
            let provider: Arc<dyn RuntimePathResolver> = Arc::new(NativeRuntimePathResolver);
            services
                .instance::<Arc<dyn RuntimePathResolver>>(Arc::new(provider))
                .map_err(provider_error)?;
        }
        if !services.contains::<Arc<dyn NetworkAddressesProvider>>() {
            let provider: Arc<dyn NetworkAddressesProvider> =
                Arc::new(NativeNetworkAddressesProvider);
            services
                .instance::<Arc<dyn NetworkAddressesProvider>>(Arc::new(provider))
                .map_err(provider_error)?;
        }
        if !services.contains::<PlatformServices>() {
            services
                .singleton::<PlatformServices, _>(|container| {
                    let process_metrics = container.get::<Arc<dyn ProcessMetricsProvider>>()?;
                    let system_paths = container.get::<Arc<dyn SystemPaths>>()?;
                    let runtime_paths = container.get::<Arc<dyn RuntimePathResolver>>()?;
                    Ok(Arc::new(PlatformServices {
                        process_metrics: Arc::clone(process_metrics.as_ref()),
                        system_paths: Arc::clone(system_paths.as_ref()),
                        runtime_paths: Arc::clone(runtime_paths.as_ref()),
                    }))
                })
                .map_err(provider_error)?;
        }
        Ok(())
    }
}

fn provider_error(error: crate::container::ContainerError) -> ProviderError {
    Box::new(error)
}

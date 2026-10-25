# Servicios de plataforma

`PlatformServices` agrupa referencias a proveedores nativos que una composición necesita, como métricas y rutas. Evita pasar adaptadores dispersos por cada constructor.

## Uso en el bootstrap

```rust
use kotavel::{
    metrics::NativeProcessMetricsProvider,
    platform_services::PlatformServices,
    system_paths::{NativeRuntimePathResolver, NativeSystemPaths},
};
use std::sync::Arc;

// La CLI o aplicación construye adaptadores e inyecta el agregado.
let services = PlatformServices {
    process_metrics: Arc::new(NativeProcessMetricsProvider),
    system_paths: Arc::new(NativeSystemPaths::default()),
    runtime_paths: Arc::new(NativeRuntimePathResolver::default()),
};
```

El núcleo define los contratos y la plataforma es la capa prevista para los
adaptadores nativos. Hoy `platform` todavía reexporta varias implementaciones
que siguen alojadas en `core`; el traslado está en curso. La aplicación compone
dependencias al arrancar, sin seleccionar el SO dentro de cada módulo.

## Campos del agregado

`PlatformServices` no crea servicios; sus campos son referencias compartidas
ya construidas:

| Campo | Trait |
| --- | --- |
| `process_metrics` | `Arc<dyn ProcessMetricsProvider>` |
| `system_paths` | `Arc<dyn SystemPaths>` |
| `runtime_paths` | `Arc<dyn RuntimePathResolver>` |

La CLI o el bootstrap construye este valor y lo registra en el contenedor. El
tipo permite que una aplicación sustituya una implementación sin alterar los
módulos del núcleo.

# Contratos

`contracts` reúne reexportaciones de traits que definen fronteras estables para filesystem, sesiones, métricas, red y rutas del sistema. Un contrato describe lo que requiere el núcleo; la implementación se proporciona mediante adaptadores.

## Ejemplo

```rust
use kotavel::contracts::NetworkAddressesProvider;

fn list_addresses(provider: &dyn NetworkAddressesProvider) {
    let _addresses = provider.local_ipv4_addresses();
}
```

Implementa el trait requerido en `platform` o en la aplicación y regístralo durante el bootstrap. Esta capa no debe incluir detalles de Qt, Axum o APIs nativas.

## Contratos exportados

| Trait o tipo | Capacidades |
| --- | --- |
| `Filesystem`, `FileCursor`, `FileLock` | Persistencia de archivos, recorrido incremental y locks |
| `ProcessMetricsProvider`, `ProcessMetricsSnapshot` | Lectura de CPU/memoria del proceso |
| `NetworkAddressesProvider` | Consulta de IPv4 locales |
| `RuntimePathResolver`, `RuntimePaths` | Recursos junto al ejecutable o bundle |
| `SessionStore` | Lectura, commit CAS, rotación, mutaciones y expiración de sesiones |
| `SessionIdentifierTransport` | Conversión de ID opaco a/desde headers y cookies |
| `SystemPaths` | Directorios de configuración, datos y caché |
| `PlatformServices` | Agregado de los servicios nativos del runtime |

Los traits son objetos seguros para compartir cuando así lo requiere el contrato
(`Send + Sync`). Implementa solo los necesarios en un adaptador de aplicación;
la biblioteca `platform` ofrece los adaptadores nativos disponibles.

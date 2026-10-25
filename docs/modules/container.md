# Contenedor de servicios

`container` es un contenedor de dependencias concurrente. Registra fábricas antes del arranque, crea singletons una vez y permite bindings transitorios. No usa reflexión: los tipos y dependencias se declaran explícitamente.

## Ejemplo

```rust
use kotavel::container::Container;
use std::sync::Arc;

struct Clock;
let services = Container::new();
services.singleton::<Clock, _>(|_| Ok(Arc::new(Clock)))?;
let clock = services.get::<Clock>()?;
services.seal()?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

Usa `get<T>()` para un singleton y `make<T>()` para una instancia transitoria. Registra servicios antes de sellar el contenedor; al cerrar, `shutdown()` libera las instancias en orden inverso de construcción.

## API del contenedor y providers

| API | Función |
| --- | --- |
| `singleton<T>(factory)` | Crear una instancia compartida al primer `get` |
| `bind<T>(factory)` | Crear una instancia nueva por `make` |
| `instance<T>(Arc<T>)` | Registrar una instancia ya construida |
| `get<T>()`, `make<T>()`, `contains<T>()` | Resolver o consultar registros |
| `seal()`, `sealed()` | Cerrar registros al iniciar la resolución |
| `shutdown()`, `stopped()` | Liberar instancias en orden inverso |

Una fábrica devuelve `Result<Arc<T>, ContainerError>`. El contenedor detecta
duplicados, dependencias circulares (también entre hilos), errores de fábrica y
resoluciones después del cierre.

Los providers implementan `register_services`, `boot` y `shutdown`.
`ProviderLifecycle::register_services()` registra en orden; `boot()` inicia
en orden después de sellar el contenedor; `shutdown()` limpia en orden inverso
incluso ante fallos parciales. Consulta el blueprint para ubicar el bootstrap.

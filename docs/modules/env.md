# Entorno

`env` lee el entorno del proceso y archivos de entorno, interpreta valores tipados y permite scopes anidados. `AppEnvironment` identifica el entorno de la aplicación; las reglas del archivo y la precedencia se validan al cargar.

## Ejemplo

```rust
use kotavel::env::{EnvironmentSource, FileEnvironment};

let source = FileEnvironment::load(".", "development")?;
let _port = source.read("APP_PORT")?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

Usa variables de entorno para valores dependientes de la máquina y secretos. Añade nombres esperados y defaults no sensibles a `.env.example`, nunca a un archivo con credenciales reales.

## API y precedencia

| API | Función |
| --- | --- |
| `EnvironmentSource::read(key)` | Puerto común de lectura |
| `ProcessEnvironment` | Leer variables del proceso |
| `FileEnvironment::load(root, environment)` | Leer el archivo de entorno del proyecto y combinarlo con el proceso |
| `Env::new(source)` | Construir lector tipado |
| `get`, `require`, `string`, `boolean` | Leer valor opcional, requerido, texto o booleano |
| `AppEnvironment::load(env)`, `load_current()` | Resolver nombre y entorno de la aplicación |
| `EnvScope::new(source)` | Instalar un scope por hilo |
| `env`, `env_string`, `env_boolean` | Helpers sobre el scope activo |

Los valores aceptan texto, booleanos, null y valores vacíos según las reglas
del parser. El proceso tiene precedencia sobre el archivo. Un archivo debe ser
UTF-8, tiene límite de tamaño, rechaza claves duplicadas y se elige por
entorno: `.env` para production, `.env.local` o `.env.development` para
esos modos. Los scopes anidados restauran la fuente anterior al salir.

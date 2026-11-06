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
| `get`, `require`, `contains`, `string`, `try_string`, `boolean`, `try_boolean`, `int`, `int_or` | Leer valores opcionales y tipados |
| `AppEnvironment::load(env)`, `load_current()` | Resolver nombre y entorno de la aplicación |
| `EnvScope::new(source)` | Instalar un scope por hilo |
| `env`, `env_string`, `env_boolean`, `env_int`, `env_int_or` | Helpers sobre el scope activo |

Los lectores `string` y `boolean` reciben un default y devuelven el valor
directamente; si el valor falta, no se puede leer o no coincide con el tipo,
devuelven el default. Usa `try_string` y `try_boolean` cuando quieras que esos
casos produzcan un error. `Env::int` requiere una variable y devuelve un error
si falta o no se puede convertir. El tipo entero se infiere del campo receptor.
`int_or` usa el default si la variable está ausente o inválida; `try_int_or`
usa el default solo si falta y devuelve error si su contenido es inválido:

```rust
let timeout = env.int("APP_TIMEOUT")?;
let retries = env.int_or("APP_RETRIES", 3_u32);
let validated_retries: u32 = env.try_int_or("APP_RETRIES", 3)?;
```

Los valores aceptan texto, booleanos, null y valores vacíos según las reglas
del parser. El proceso tiene precedencia sobre el archivo. Un archivo debe ser
UTF-8, tiene límite de tamaño, rechaza claves duplicadas y se elige por
entorno: `.env` para production, `.env.local` o `.env.development` para
esos modos. Los scopes anidados restauran la fuente anterior al salir.

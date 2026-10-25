# Configuración

El módulo `config` carga archivos de configuración registrados y expone valores JSON bajo claves con puntos. La aplicación decide qué archivos registrar; el framework no escanea carpetas implícitamente.

## Qué puedes hacer

- Leer valores como texto, booleano, entero o estructura JSON.
- Cargar varios archivos y fusionar sus grupos en orden.
- Aplicar cambios por lote de forma atómica.
- Crear un scope temporal de configuración para una operación.

## Ejemplo

```rust
use kotavel::config::{Config, ConfigValue};

let mut config = Config::new();
config.set("http.port", ConfigValue::from(8080))?;
let port: u64 = config.get_typed("http.port")?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

Los nombres exactos de consultas tipadas y los errores están en la API pública de `config`. Mantén secretos en el entorno y valores predeterminados no sensibles en los archivos registrados.

## API disponible

| API | Qué permite |
| --- | --- |
| `Config::new()` | Crear un mapa vacío |
| `set(key, value)`, `set_many(updates)` | Escribir un valor o aplicar cambios atómicos |
| `get(key)`, `value(key)`, `get_typed<T>(key)` | Consultar opcional, obligatorio o deserializado |
| `contains(key)`, `iter()` | Comprobar presencia y recorrer claves |
| `group(prefix)`, `get_many(keys)` | Recuperar grupos anidados o varias claves |
| `merge(group, map)` | Agregar un grupo validado |
| `ConfigFile`, `load_config_files(files)` | Cargar archivos registrados en orden |
| `with_config_scope`, `config`, `config_get`, `config_get_many`, `config_set` | Acceder al scope activo en callbacks sincronizados |

Las claves son grupos separados por puntos. Las escrituras por lote y las
fusiones validan primero y no dejan cambios parciales si hay colisión o valor
inválido. Los getters tipados usan deserialización Serde.

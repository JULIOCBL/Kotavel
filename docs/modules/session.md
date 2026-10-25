# Sesiones

El módulo `session` gestiona sesiones por request, expiración, rotación de
identificador, cache concurrente, persistencia SQLite WAL y contexto compartido
enlazado explícitamente a una sesión autenticada.

## Flujo básico

```rust
use kotavel::session::{SessionConfig, SessionManager, SessionSettings};
use kotavel::env::{Env, FileEnvironment};
use std::sync::Arc;

let source = FileEnvironment::load(".", "development")?;
let settings = SessionSettings::load(&Env::new(&source))?;
let manager = Arc::new(settings.create_manager(".")?);

let mut session = manager.begin(None)?;
session.put("user_id", serde_json::json!(42))?;
let committed = session.commit()?;
# let _ = committed;
# Ok::<(), Box<dyn std::error::Error>>(())
```

Un request no retiene un lock mientras ejecuta el handler: `begin` carga una
instantánea y el scope conserva un overlay de cambios. `commit` aplica una
revisión compare-and-swap; incrementos explícitos pueden rebasarse sobre
actualizaciones concurrentes sin perder contadores. Una colisión de escrituras
normales puede devolver `false`, y el consumidor decide si reintenta o responde
conflicto.

## SessionManager y SessionScope

`SessionManager::new(store, config)` permite inyectar cualquier
`SessionStore`; `SessionManager::sqlite(path, config, store_options,
cache_options)` construye SQLite y cache. `begin(candidate_id)` restaura solo
IDs válidos y activos; IDs ausentes, inválidos o expirados crean una sesión
nueva. `collect_expired()` elimina sesiones vencidas.

| Operación en `SessionScope` | Función |
| --- | --- |
| `id()` | Consultar el ID opaco actual |
| `get(key)`, `has(key)`, `exists(key)`, `all()` | Leer valores, presencia o mapa |
| `put(key, value)`, `remove(key)`, `forget(key)` | Guardar o eliminar valores |
| `increment(key, amount)` | Incremento entero rebajable ante contención |
| `csrf_token()`, `verify_csrf_token(value)` | Crear y validar token CSRF por sesión |
| `flush()` | Borrar valores con mutación exclusiva |
| `regenerate()` | Rotar ID y conservar datos |
| `invalidate()` | Rotar ID y vaciar datos |
| `commit()` | Confirmar overlay; devuelve false ante conflicto |

Los registros contienen created_at, last_activity, expires_at, revision y JSON
de valores. El ID usa 256 bits aleatorios (64 caracteres hexadecimales);
`valid_session_id` valida el formato y `generate_session_id` lo crea.

## Almacenes y cache

`SessionStore` define `load`, `load_snapshot`, `commit`,
`commit_rotation`, `commit_mutation`, `remove` y `collect_expired`.
Implementa el trait para cambiar persistencia sin modificar manager o
middleware.

`SQLiteSessionStore::open(path, StoreOptions)` habilita WAL, pool de
conexiones de lectura y un writer dedicado con cola acotada. Las transacciones
confirman revisiones CAS y permiten agrupar escrituras en una ventana de hasta
10 ms. `path()` y `statistics()` exponen ruta y estadísticas de cola,
espera, ejecuciones y lotes. Opciones: conexiones de lectura, capacidad de cola,
busy timeout y ventana de batching.

`CachedSessionStore::new(backing, CacheOptions)` agrega cache read-through
shardeada. Se configura con máximo de sesiones, bytes, TTL y shards (1–1024).
`statistics()` reporta hits, misses, evictions, ocupación y contención/espera
de locks. El store durable conserva autoridad de revisiones; snapshots obsoletas
no reemplazan versiones recientes y el límite global de residentes se controla
entre shards.

## Settings y transporte

`SessionSettings::from_config(config)` y `load(env)` validan las opciones.
También ofrece `create_store(root)`, `create_manager_with_store(store)`,
`create_manager(root)` y `resolved_database_path(root)`.

Variables principales:

| Variable | Propósito |
| --- | --- |
| `SESSION_DRIVER`, `SESSION_DATABASE` | Driver SQLite y archivo |
| `SESSION_LIFETIME`, `SESSION_MAX_LIFETIME` | Vida deslizante y absoluta, en minutos |
| `SESSION_MAX_BYTES` | Límite de datos por sesión |
| `SESSION_CACHE_MAX_SESSIONS`, `SESSION_CACHE_MAX_BYTES`, `SESSION_CACHE_SHARDS`, `SESSION_CACHE_TTL_MS` | Presupuesto de cache |
| `SESSION_READ_CONNECTIONS`, `SESSION_WRITE_QUEUE_CAPACITY`, `SESSION_WRITE_BATCH_WINDOW_US` | Lecturas concurrentes y cola de escritura |
| `SESSION_TRANSPORT` | `cookie` o `header` |
| `SESSION_COOKIE`, `SESSION_PATH`, `SESSION_DOMAIN`, `SESSION_SECURE_COOKIE`, `SESSION_HTTP_ONLY`, `SESSION_SAME_SITE` | Atributos de cookie |
| `SESSION_HEADER` | Nombre del header si el transporte es header |

`SessionIdentifierTransport::cookie(options)` valida nombre, path, domain y
política SameSite/Secure. `header(name)` valida el nombre del header. Sus
métodos `read`, `response_header`, `input_name` e `is_cookie` traducen el
ID opaco sin aceptar IDs caller-controlled; los valores inválidos se ignoran.

Solo está implementado el driver SQLite; `SESSION_DRIVER` distinto de `sqlite`
se rechaza. `SESSION_ENCRYPT=true` también se rechaza porque el cifrado aún no
está implementado. `SESSION_MAX_LIFETIME=0` desactiva el límite absoluto; la
vida deslizante y el tamaño máximo deben ser mayores que cero. La ventana de
batch admite 0–10.000 µs; las conexiones de lectura y la cola no pueden ser 0.
`SameSite=None` exige `SESSION_SECURE_COOKIE=true`. La ruta de base relativa se
resuelve desde la raíz del proyecto consumidor.

## Sesiones HTTP

`start_session(manager, transport)` crea middleware por request;
`start_session_with_context(manager, transport, resolver)` añade el acceso al
contexto vinculado. Instálalo solo en los grupos que necesiten estado. En el
handler, `request.session()` permite `id/get/get_value/has/exists/all/put/
forget/increment/flush/csrf_token/verify_csrf_token/regenerate/invalidate`.
Sin el middleware, el acceso devuelve `SessionAccessError::Unavailable`.

## Contexto compartido y grants

El contexto compartido es distinto de la sesión privada:

1. `SessionContextResolver` crea/default_context/elimina contextos y guarda
   valores con duración opcional.
2. `SessionContext::issue_link_grant_for(lifetime)` crea un grant temporal y
   de un solo uso.
3. El usuario autenticado canjea el grant con su sesión HTTP. Esa sesión queda
   vinculada a un contexto.
4. `LinkedSessionContext::get/put/revoke` vuelve a comprobar el vínculo en cada
   operación; una revocación se aplica inmediatamente.

El store limita número de contextos, grants y bytes. No compartas datos entre
sesiones solo por conocer su ID; requiere autorización explícita mediante el
grant y revoca el vínculo al cerrar el acceso.

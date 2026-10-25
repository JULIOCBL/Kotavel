# Migración de Kotavel a Rust

El árbol vigente del crate y sus reglas de organización están documentados en
el [blueprint de estructura Rust](blueprint/estructura.md). El blueprint
propuesto original conserva el diseño histórico C++.

La raíz del repositorio contiene el núcleo implementado en Rust.
Las fuentes C++ originales se retiraron; la documentación histórica se conserva.

## Comandos con Cargo

Desde esta raíz, `cargo kotavel` muestra la ayuda del CLI. Ejecuta sus
subcomandos con `cargo kotavel <comando>`, por ejemplo `cargo kotavel test` o
`cargo kotavel build`.

## Objetivo que se conserva

La migración mantiene el comportamiento y los contratos de Kotavel: un runtime
modular para Desktop, HTTP y Web, con el motor de sesiones de alta concurrencia
como requisito central. Se conservarán aislamiento por sesión, persistencia,
expiración, rotación y revocación, vinculación explícita de contextos, carga
acotada y mediciones reproducibles.

## Mapa de traslado

| Módulo C++ con implementación | Módulo Rust |
| --- | --- |
| `foundation`, `container`, `config`, `env` | `foundation`, `container`, `config`, `env` |
| `filesystem` y contratos de almacenamiento | `filesystem`, `contracts` |
| `session` y contratos de sesión | `session`, `contracts` |
| `http` (kernel, rutas, request, validación, sesión) | `http`; el Laboratorio conecta Axum como adaptador |
| `desktop`, Qt/QML | `desktop` y adaptador `desktop-qt` opcional |
| `system`, `metrics`, `diagnostics` | `system`, `metrics`, `diagnostics` |
| `console`, `text` | `console`, `text` |
| herramientas de desarrollo y pruebas | CLI Rust, pruebas unitarias, integración y benchmark |

Los directorios C++ `events`, `queue`, `cache` y `database` son marcadores vacíos
en esta versión; no contienen implementación funcional que trasladar. Los
adaptadores externos (HTTP, Qt, sistemas operativos y SQLite) se aíslan para
mantener el núcleo multiplataforma.

## Estado

El workspace Cargo implementa los contratos de filesystem y sesión,
el almacén SQLite WAL, la caché particionada con límites, IDs aleatorios de 256
bits, el ámbito por solicitud con CAS/rebase para contadores, la configuración
por claves jerárquicas y el acceso acotado al entorno del proceso y archivos
`.env`, además de un contenedor de dependencias tipado con ciclos detectables y
ciclos de vida singleton/transient, las métricas de proceso y su monitor, la
identidad estable del sistema operativo y la fachada de locale por hilo de
`foundation`. `contracts/` reúne puertos compartidos para filesystem, sesiones,
métricas, rutas del sistema y runtime, y direcciones de red. `system/` implementa
la enumeración nativa de IPv4 activa en Unix y Windows. `metrics/` ya expone un
stream JSON Lines solo en loopback con descriptores por PID y limpieza de
endpoints obsoletos. `text/` carga los
catálogos incorporados y valida la sustitución literal de parámetros. El filesystem separa
su contrato del adaptador local, con escritura atómica, locks, límites de lectura
y permisos nativos Unix/Windows; SQLite ya usa el mismo manejo de permisos para
las sesiones. `diagnostics/` aporta errores tipados, contexto validado, causas
limitadas, captura portátil de backtrace, presentación segura por entorno, un
logger concurrente con filtros, throttling acotado y rotación, y logging de
arranque anterior a `Application::boot()`. `foundation/` coordina el ciclo de vida
síncrono de `Application` con las políticas de error y logging. El núcleo HTTP
ya ofrece un kernel independiente del transporte y un servidor reemplazable; el
Laboratorio lo conecta mediante Axum. `desktop/` ya ofrece el router gráfico,
la ruta inicial, deep links validados y contratos para ventanas y plataformas;
la resolución de vistas QML canonicaliza la raíz y rechaza enlaces simbólicos
que salen de ella. `desktop-qt` conecta ese contrato con Qt Quick/QML mediante
el puente Rust/C++; inicia el loop nativo, carga vistas y enruta deep links. El
adaptador tiene una prueba de arranque `offscreen`; las pruebas normales del
núcleo siguen funcionando sin Qt ni una sesión gráfica.

El writer SQLite de Rust acepta una ventana de agrupación de hasta 10 ms. Cada
commit espera la confirmación durable; los savepoints aíslan errores de una
operación para preservar las demás del lote. El Laboratorio configura esa ventana
con `SESSION_WRITE_BATCH_WINDOW_US`.

`CachedSessionStore` usa locks independientes por shard y reservas atómicas para
respetar `max_sessions` y el límite agregado `max_bytes` aun cuando la carga se
concentre en algunos shards. `max_bytes` contabiliza el JSON serializado, el ID y
el tamaño base de `SessionRecord`; representa un presupuesto aproximado de caché,
no el overhead interno del allocator.

`session::SessionContext` conserva el contexto compartido tras el puerto
`SessionContextStore` (SQLite es la implementación incluida). Una aplicación
confiable puede emitir un grant de un solo uso; el middleware HTTP puede
canjearlo por la sesión actual y solo esa sesión obtiene acceso a los valores:

```rust
let resolver = Arc::new(SessionContextResolver::new(Arc::clone(&store)));
let context = SessionContext::default_for(Arc::clone(&resolver))?;
let grant = context.issue_link_grant()?;
resolver.redeem_link_grant(&grant, authenticated_session_id)?;
let shared_value = resolver.get_for_session(authenticated_session_id, "company.id")?;
```

Para integrar esa capacidad con el ciclo HTTP, instala el middleware de sesión
con el mismo resolver. Las rutas obtienen acceso al contexto solo después de
canjear el grant; `LinkedSessionContext` vuelve a validar el vínculo al leer o
escribir, de modo que una revocación afecta también a handles ya obtenidos:

```rust
kernel.use_api(start_session_with_context(
    Arc::clone(&manager),
    transport,
    Arc::clone(&resolver),
))?;

// Dentro de la ruta que recibe el grant de la aplicación confiable:
request.redeem_session_context_link(&grant)?;
let context = request.session_context()?.expect("grant was redeemed");
let company_id = context.get("company.id")?;
```

La ruta de canje debe estar protegida por la autorización de la aplicación y
aceptar grants solo desde el flujo confiable que los emitió. El middleware nunca
vincula automáticamente sesiones nuevas o remotas.

El kernel Desktop puede recibir explícitamente el mismo contexto operativo.
Sus rutas leen y escriben datos compartidos con `context.session_context()`;
la sesión HTTP sigue teniendo un ID independiente y solo queda enlazada cuando
el servidor canjea un grant autorizado:

```rust
let store = Arc::new(SQLiteSessionStore::open(
    "storage/sessions.sqlite3",
    StoreOptions::default(),
)?);
let resolver = Arc::new(SessionContextResolver::new(Arc::clone(&store)));
let context = Arc::new(SessionContext::default_for(Arc::clone(&resolver))?);
context.put("company.id", json!(15), None)?;

let mut desktop = DesktopKernel::new(platform)
    .with_session_context(Arc::clone(&context));
// Dentro de una ruta Desktop:
let company_id = desktop_context
    .session_context()
    .expect("desktop session context is configured")
    .get("company.id")?;

let grant = context.issue_link_grant()?;
resolver.redeem_link_grant(&grant, authenticated_web_session_id)?;
```

Los controladores HTTP Rust conservan una instancia compartida entre handlers y
permiten adaptar acciones normales o acciones con validación tipada previa:

```rust
let controller = Controller::new(ItemController::default());
let handler = controller.form_action(
    StoreItemRequest::from_request,
    |controller, form, validation| controller.store(form, validation),
);
Route::api(&mut router).post("/items", handler)?;
```

Sin `with_session_context`, las rutas Desktop no reciben datos compartidos.

`SessionScope` también expone `regenerate()` para reemplazar el ID conservando
los valores, e `invalidate()` para reemplazarlo y limpiar la sesión. Ambos
retiran el ID anterior dentro de la misma transacción SQLite y renuevan el token
CSRF anterior para que `csrf_token()` genere uno nuevo; si dos solicitudes
intentan rotar la misma revisión, solo una puede guardar.

El cliente de métricas del CLI también está portado como binario Rust:

```sh
cargo run --bin kotavel-monitor -- --project /ruta/al/proyecto
cargo run --bin kotavel-monitor -- --project /ruta/al/proyecto --pid 1234
```

El cierre del CLI también está portado. Busca los endpoints de métricas activos
del proyecto, verifica que cada PID pertenezca a un ejecutable dentro de
`build/` y solicita un cierre ordenado antes de forzarlo:

```sh
cargo run --bin kotavel-stop -- --project /ruta/al/proyecto
cargo run --bin kotavel-stop -- --project /ruta/al/proyecto --pid 1234
```

El benchmark de sesiones también está disponible como binario Rust. Ejecuta
la carga ofrecida a 2 solicitudes por cliente/s y 20% de escrituras en tres
perfiles (sesiones independientes, una sesión compartida o una sesión con
claves independientes), reporta latencias p50/p95/p99, throughput, uso de la
caché, locks contenciosos por shard, espera del writer y del cache, CPU y RSS;
además verifica que los contadores durables recuperados desde SQLite coincidan
con las escrituras aceptadas:

```sh
cargo run --release --bin kotavel-session-benchmark -- 500 10 independent 0
cargo run --release --bin kotavel-session-benchmark -- 500 10 shared 0
cargo run --release --bin kotavel-session-benchmark -- 500 10 shared-keys 1000
# También está disponible desde el CLI principal:
cargo run --bin kotavel -- benchmark --release 500 10 shared 0
# Tres repeticiones, validación de carga y reporte JSON con hash de fuentes:
python3 benchmarks/run_session_benchmark.py target/release/kotavel-session-benchmark \
  --clients 500 --seconds 10 --repeats 3 --build "Release macOS ARM64" \
  --output benchmarks/baselines/session-rust-acceptance.json
```

La medición cubre el gestor de sesiones en proceso, no sockets HTTP. La espera
del writer incluye contrapresión al admitir trabajos y el intervalo de
agrupación configurado; el servicio máximo se mide por trabajo o transacción
agrupada. La contención del cache cuenta solo locks cuyo `try_lock` no pudo
adquirirse al primer intento. CPU y RSS se muestrean cada 100 ms, por lo que el
CSV también registra el número de muestras. Para medir la ruta HTTP completa se
debe usar el adaptador de Laboratory y una herramienta de carga de red.

La referencia de tres repeticiones para macOS ARM64 está en
[`benchmarks/baselines/session-rust-acceptance-2026-10-25.json`](../benchmarks/baselines/session-rust-acceptance-2026-10-25.json).
Las nueve corridas completaron 5.000 solicitudes y 1.000 escrituras cada una;
cache y SQLite confirmaron todos los contadores, sin conflictos ni fallos. La
mediana observada fue 998,37–998,75 solicitudes/s y p95 de 0,300 ms para sesiones
independientes, 0,300 ms para una clave compartida y 0,885 ms para claves
independientes en una sesión. Son mediciones de esta máquina y de carga en
proceso, no un umbral general de producción.

`system/` resuelve directorios estándar y
recursos de runtime con los layouts de Windows, macOS y Linux; `metrics/` obtiene
CPU y memoria residente con adaptadores nativos para esos sistemas.

`console/` porta el análisis de opciones, la conversión tipada de `logging.*` y
`metrics.*`, el arranque de `Application`, la ejecución del consumidor y el
ciclo ordenado de proveedores nativos y del consumidor. `container/` separa
estado, errores y resolución concurrente; usa
fábricas tipadas y ciclos de vida singleton/transient. `env/` define el puerto
`EnvironmentSource` y separa los adaptadores de proceso y archivo, valores
tipados, ajustes de aplicación y errores; y
`config/` separa almacenamiento, límites y validación, carga de registros y
helpers con alcance de configuración aislado por hilo. `foundation/` separa la
fachada de aplicación y su locale; `contracts/` contiene los puertos compartidos;
`system/` identifica la plataforma y separa los resolvers de rutas nativas;
`diagnostics/` separa contexto,
errores, presentación y logging. `text/` aloja los catálogos de `resources/messages/`
y el formateador con errores tipados. `filesystem/` separa el contrato, el adaptador de
disco y el código de cada plataforma. El crate del núcleo vive directamente en
`Kotavel/`.

## Rutas estilo Laravel

La API conserva conceptos familiares para quienes usan Laravel: `Route::web`,
`Route::api`, `get`, `post`, `put`, `patch`, `delete`, `prefix`, `middleware` y
`group`. El compilador de Rust señala los errores de configuración al arrancar.

```rust
let mut web = Route::web(kernel.routes_mut()?);
web.get("/", home_handler)?.name("web.home")?;

Route::api(kernel.routes_mut()?)
    .prefix("items")?
    .middleware_alias("api.auth")?
    .group(|items| {
        items.get("/", list_items)?.name("api.items.index")?;
        items.post("/", create_item)?.name("api.items.store")?;
        Ok(())
    })?;
```

Los grupos heredan prefijos y middleware; las rutas pueden declarar nombres y
middleware propios. `remove` sigue disponible como alias de `delete`.

## Middleware de sesiones

El kernel permite activar sesiones solo en las rutas web que las necesitan. El
middleware abre el ámbito por solicitud, lo expone al handler mediante
`request.session()`, guarda los cambios al terminar y devuelve el ID con la
cookie o cabecera configurada. La fachada mantiene operaciones de sesión cortas
y tipadas sin exponer referencias fuera del bloqueo:

```rust
let manager = Arc::new(SessionManager::sqlite(
    "storage/sessions.sqlite3",
    SessionConfig::default(),
    StoreOptions::default(),
    CacheOptions::default(),
)?);
let sessions = start_session(
    Arc::clone(&manager),
    SessionIdentifierTransport::cookie(CookieTransportOptions::default())?,
);
kernel.use_web(sessions)?;

let profile: Handler = Arc::new(|request| {
    let visits = request.session().and_then(|session| {
        session.increment("visits", 1)?;
        session.get::<u64>("visits")
    });
    match visits {
        Ok(Ok(value)) => ApiResponse::success(&value).unwrap(),
        _ => Response::new(500, "text/plain; charset=utf-8", "Session unavailable"),
    }
});
```

Para clientes sin cookies se puede configurar `SessionIdentifierTransport::header`.
Los conflictos de escritura concurrente devuelven HTTP 409 para que el cliente
reintente con el estado actualizado. El mantenimiento de sesiones expiradas se
ejecuta en segundo plano.

La prueba de concurrencia `tests/session_concurrency.rs` cubre dos cargas
complementarias: 500 clientes con sesiones independientes y 20% de escrituras,
y 64 workers que completan 2,048 commits sobre el mismo ID mientras actualizan
un contador común y contadores independientes. En ambas cargas se comprueba el
valor recuperado desde la persistencia después de terminar las solicitudes.

## CLI Rust

El binario `kotavel` ya ofrece preparación del entorno Rust, descarga de
dependencias, pruebas, compilación, ejecución, monitor y cierre. `prepare`
comprueba `rustc`, Cargo y el manifiesto del workspace; `install` ejecuta
`cargo fetch` para el proyecto seleccionado. `test --framework` valida el
núcleo y el Laboratorio Rust local en una sola ejecución:

```sh
cargo run --bin kotavel -- test
cargo run --bin kotavel -- --project /ruta/al/proyecto test --framework
cargo run --bin kotavel -- prepare
cargo run --bin kotavel -- install
cargo run --bin kotavel -- build --release
cargo run --bin kotavel -- run
cargo run --bin kotavel -- shell
cargo run --bin kotavel -- monitor --pid 1234
cargo run --bin kotavel -- stop --pid 1234
```

`run` ejecuta desde la raíz del consumidor y entrega esa ruta al runtime para
cargar su `.env`, recursos y vistas. También acepta `--env` y `--deep-link` con
una sintaxis familiar para el CLI anterior:

```sh
cargo run --bin kotavel -- --project /ruta/al/proyecto run \
  --env development --deep-link kotavel://pos/home
```

Si el crate define varios binarios, selecciona uno con `--bin nombre`; el
núcleo declara `kotavel` como binario predeterminado. Los argumentos después de
`--` se entregan directamente a la aplicación.

`install` descarga las dependencias del `Cargo.toml`; para declarar una nueva,
usa la sintaxis de Cargo:

```sh
cargo run --bin kotavel -- --project /ruta/al/proyecto install add serde@1 --features derive
cargo run --bin kotavel -- --project /ruta/al/proyecto install --offline
cargo run --bin kotavel -- --project /ruta/al/proyecto install --reset
```

`--reset` limpia los artefactos Cargo del proyecto y vuelve a descargar las
dependencias declaradas. Se puede combinar con `--offline` si ya están en caché.

`shell` valida el workspace y abre la terminal interactiva nativa en la raíz del
proyecto, con `KOTAVEL_PROJECT_ROOT` disponible para las herramientas lanzadas
desde ella. La CLI Rust también puede compilar un binario Cargo y crear un
bundle `.app` macOS; puede generar además imágenes `.dmg` e instaladores `.pkg`.
Las vistas QML, el archivo `.env` y el nombre del entorno se copian a Recursos.
Al iniciar el `.app`, el runtime lee esos recursos, usa las vistas empaquetadas y
guarda los logs bajo `~/Library/Application Support/<App>/logs`.

El adaptador Qt es opcional. Con Qt 6, CMake y un compilador C++17 disponibles:

```sh
cargo check --features desktop-qt
QT_QPA_PLATFORM=offscreen cargo test --features desktop-qt
```

Para empaquetar un consumidor Cargo en macOS (requiere el archivo de entorno
seleccionado en la raíz del proyecto):

```sh
cargo run --bin kotavel -- --project /ruta/al/proyecto package \
  --format all --env-file .env --features desktop-qt --output dist
```

`--format` acepta `app`, `dmg`, `pkg` o `all`; `--bin` selecciona un binario si
el paquete Cargo define varios. La firma ad hoc se verifica antes de publicar
el directorio de salida. `--deploy-qt` ejecuta `macdeployqt` para aplicaciones
Qt; se activa automáticamente al incluir la feature `desktop-qt`.

Para compilar y ejecutar las pruebas desde la raíz del repositorio:

```sh
cargo check --workspace
cargo test --workspace
```

Desde la raíz del repositorio, el mismo workspace se valida con:

```sh
cargo test --workspace
```

# Sesiones

Kotavel almacena las sesiones con SQLite en modo WAL, en una base de datos
ubicada por defecto en:

```text
<raíz del proyecto>/storage/framework/sessions.sqlite3
```

La API pública vive en `<kotavel/session/session.hpp>`; el include anterior
`<kotavel/http/session.hpp>` se conserva como compatibilidad. El middleware
HTTP está declarado en `<kotavel/http/session_middleware.hpp>`. La sesión se
habilita explícitamente en el grupo web del `HttpKernel`:

```cpp
kotavel::FileEnvironment environment_file(project_root, "local");
kotavel::Env environment(environment_file);
kotavel::EnvScope environment_scope(environment);
const kotavel::ConfigFile files[] = {
    {"session", kotavel::http::SessionConfig::load},
};
const auto configuration = kotavel::load_config_files(files);
auto session_config = kotavel::http::SessionConfig::from_config(configuration);

kernel.use_web(kotavel::http::start_configured_session(session_config, project_root));
```

En el Laboratorio, `config/session.cpp` define las opciones y sus defaults; el
registro central `config/app_config_files.cpp` lo carga bajo el grupo `session`.
Puedes cambiar ahí los defaults de esa aplicación. El núcleo mantiene
`SessionConfig::from_config()` para convertir ese grupo en opciones del
middleware. `SESSION_*` puede sobrescribir los valores definidos en el archivo.

Dentro de un handler o middleware:

```cpp
kotavel::http::Session::put("user_id", 42);
const auto user_id = kotavel::http::Session::get<int>("user_id", 0);

// Forma equivalente ligada a la petición actual:
kotavel::http::session().put("theme", "dark");

// Actualización atómica para contadores concurrentes:
kotavel::http::Session::increment("cart.count");
```

`Request::session()` usa la misma sesión. La API solo está disponible mientras
la petición atraviesa el middleware de sesión; su uso fuera de ese ámbito
produce `std::logic_error`.

`Session::increment()` trata una clave ausente como cero y acepta solo enteros
no negativos. El motor rebasa incrementos concurrentes sobre la última revisión
durable. `get()` dentro de una petición lee su snapshot más los cambios locales;
si otros handlers incrementan la misma clave al mismo tiempo, una lectura en el
handler puede no reflejar aún esos incrementos concurrentes. El total duradero sí
incluye cada incremento aceptado. Para `get()` seguido de `put()` sobre la misma
clave, las solicitudes obsoletas reciben 409; los cambios a claves distintas
pueden combinarse, pero `flush()`, la rotación y la invalidación conservan
semántica exclusiva.

## Configuración

Los valores se leen de `Env` y pueden definirse en el entorno o en el `.env` del
proyecto. Los defaults son:

| Variable | Default | Uso |
| --- | --- | --- |
| `SESSION_DRIVER` | `sqlite` | Único driver soportado |
| `SESSION_DATABASE` | `storage/framework/sessions.sqlite3` | Archivo SQLite relativo a la raíz del proyecto |
| `SESSION_LIFETIME` | `120` | Minutos de inactividad permitidos |
| `SESSION_MAX_LIFETIME` | `1440` | Duración máxima absoluta en minutos; `0` la desactiva |
| `SESSION_CACHE_MAX_SESSIONS` | `2048` | Máximo de sesiones residentes en la caché |
| `SESSION_CACHE_MAX_BYTES` | `33554432` | Presupuesto total aproximado de caché (32 MiB) |
| `SESSION_CACHE_SHARDS` | `64` | Particiones de caché (rango: 1–1024) |
| `SESSION_CACHE_TTL_MS` | `1000` | Antigüedad máxima de una entrada antes de releer SQLite |
| `SESSION_MAX_BYTES` | `1048576` | Tamaño aproximado máximo de los datos de una sesión (1 MiB) |
| `SESSION_READ_CONNECTIONS` | `4` | Conexiones SQLite de lectura reutilizables |
| `SESSION_WRITE_QUEUE_CAPACITY` | `1024` | Máximo de escrituras pendientes antes de aplicar contrapresión |
| `SESSION_WRITE_BATCH_WINDOW_US` | `0` | Microsegundos máximos para agrupar commits cercanos; `0` usa solo commits ya en cola |
| `SESSION_COOKIE` | `kotavel_session` | Nombre de la cookie |
| `SESSION_PATH` | `/` | Ruta de la cookie en el navegador |
| `SESSION_DOMAIN` | sin valor | Dominio de la cookie |
| `SESSION_SECURE_COOKIE` | `false` | Añade el atributo `Secure` |
| `SESSION_HTTP_ONLY` | `true` | Añade el atributo `HttpOnly` |
| `SESSION_SAME_SITE` | `lax` | `lax`, `strict` o `none` |
| `SESSION_EXPIRE_ON_CLOSE` | `false` | Omite `Max-Age` para usar una cookie de sesión |
| `SESSION_ENCRYPT` | `false` | Si se activa, el arranque falla porque el cifrado aún no está implementado |

`SESSION_PATH` configura el alcance de la cookie, no una ruta de almacenamiento.
`SESSION_DATABASE` selecciona la base de datos SQLite. El arranque configurado
crea SQLite WAL y una caché acotada; las API permanecen sin sesión a menos que
se les agregue el middleware explícitamente.

La expiración combina inactividad y edad absoluta: cada petición puede renovar
`SESSION_LIFETIME`, pero el límite `SESSION_MAX_LIFETIME` se calcula desde la
creación original y no se renueva. Al llegar a cualquiera de los dos límites,
el servidor elimina el registro y emite una sesión vacía con un ID nuevo.
Establecer `SESSION_MAX_LIFETIME=0` conserva únicamente la expiración por
inactividad.

El driver crea identificadores de 256 bits con el generador criptográfico del
sistema operativo. El navegador recibe el identificador; SQLite conserva los
datos y sus marcas de tiempo con sincronización durable. El handler no retiene
bloqueos del motor de sesiones durante su ejecución. Cada registro conserva una
revisión. El commit usa CAS en el camino sin contención. Si la revisión ya
avanzó, SQLite carga la última versión y combina el delta dentro de la misma
transacción cuando las claves afectadas siguen independientes; cambios
solapados y transiciones de identidad basadas en una revisión obsoleta se
rechazan con HTTP 409. Las lecturas que
no modifican datos no reescriben el registro en cada petición: la actividad y la
expiración se renuevan como máximo cada minuto (o antes para sesiones breves).

La rotación de ID en SQLite valida la revisión anterior, crea la nueva identidad,
retira la anterior y revoca su vínculo de contexto dentro de una sola transacción.
Así una petición obsoleta no puede recrear el ID retirado después de una
regeneración concurrente. Los stores personalizados pueden implementar la
capacidad opcional `SessionRotationStore`; el fallback conserva compatibilidad,
pero solo SQLite garantiza esta transición de forma atómica y durable.

Un único worker administra las escrituras SQLite. Agrupa los commits y merges
atómicos que ya están pendientes en la cola y espera hasta `SESSION_WRITE_BATCH_WINDOW_US` para
reunir más (hasta 64 por transacción), con un savepoint por commit. Cada llamada
espera al `COMMIT` durable antes de responder; si un savepoint falla, se revierte
esa operación sin descartar las demás del lote. La cola limitada aplica
contrapresión cuando se llena.

`kotavel.session.crash_recovery` valida esa garantía entre procesos: confirma un
registro, termina el proceso escritor sin ejecutar destructores y comprueba en
un proceso nuevo que SQLite recupera la revisión y sus valores desde WAL.

`start_configured_session()` usa una caché LRU aproximada, particionada en
`SESSION_CACHE_SHARDS` shards. El límite de sesiones se reparte entre shards;
si ese límite es menor que el número configurado de shards, reduce los shards
efectivos para que cada sesión permitida pueda ocupar una partición. El límite
de bytes es global y una partición puede usar capacidad ociosa de las demás sin
superar el máximo total. Las entradas caducan por TTL para acotar la
visibilidad de cambios hechos por otro proceso. Las escrituras invalidan la
entrada local y la siguiente lectura vuelve a cargar la revisión durable.

Las lecturas calientes obtienen un snapshot inmutable compartido: no copian el
mapa completo de valores por petición. Cada petición conserva solo los cambios
de sus claves y materializa el mapa al devolver `all()`, rotar la sesión o
persistir una modificación. Los stores personalizados conservan el contrato
`SessionStore`; la capacidad opcional `SessionSnapshotStore` permite a una
caché devolver snapshots compartidos sin añadir métodos virtuales al contrato
existente.

Las estadísticas de caché también reportan cuántas adquisiciones encontraron
contención, la espera acumulada y la espera máxima por los locks de shard. El
camino sin contención usa `try_lock` y no toma timestamps; la espera se mide
solo cuando hay que bloquear. El benchmark incluye estas métricas junto con
cola SQLite, CPU y RSS.

SQLite programa la revisión de sesiones expiradas en la primera solicitud con
sesión, cada 100 solicitudes o al transcurrir un minuto, y también rechaza una
sesión vencida cuando el navegador vuelve a presentarla. Un worker de
mantenimiento elimina registros en lotes acotados desde la tabla, fuera del
hilo de la solicitud. El contador pertenece a cada instancia de middleware y
se comparte entre sus copias. Mientras un barrido esté en ejecución, otras
peticiones de esa instancia no inician barridos adicionales. La protección se
libera incluso si falla la recolección.

Cada pasada elimina como máximo 256 sesiones vencidas, grants y valores de
contexto por tabla. La emisión de un grant también limita a 256 filas su
limpieza oportunista de grants globales vencidos; el índice por vencimiento
evita un recorrido completo de la tabla.

La sesión de aplicación inicia vacía. Las marcas de tiempo y la versión del
formato son metadatos del registro, no valores visibles mediante
`Session::all()`. Los mensajes flash y la URL anterior se agregarán cuando
existan sus componentes de navegación correspondientes.

## Contexto compartido Desktop / API / Web

`kotavel::session::Context` almacena datos operativos durables que pueden usar
Desktop y un cliente HTTP autorizado. No comparte credenciales: Desktop conserva
un `ContextId`, cada cliente HTTP conserva su propio `SessionID`, y el almacén
mantiene la relación solo después de canjear un `LinkGrant`.

Desktop crea y conserva un contexto con el mismo archivo SQLite de la aplicación:

```cpp
auto store = std::make_shared<kotavel::http::SQLiteSessionStore>(database_path);
auto resolver = std::make_shared<kotavel::session::ContextResolver>(store);
auto context = std::make_shared<kotavel::session::Context>(
    kotavel::session::Context::create(resolver));
context->put("company.id", 15);

kotavel::desktop::DesktopKernel kernel(std::move(platform), views_root, context);
```

El handler desktop recibe ese objeto mediante `DesktopContext::session_context()`.
El handler web puede resolver datos del contexto de su propia sesión:

```cpp
const auto context = resolver->get_for_session(kotavel::http::Session::id(), "company.id");
```

`get_for_session()` valida el vínculo, la sesión y el TTL del valor en una sola
consulta SQLite. Una lectura nueva que comienza después de revocar el vínculo no
puede reutilizar un `ContextId` obtenido antes en una consulta separada.

El flujo de vinculación debe comenzar tras una acción explícita de autorización
en Desktop. El código local emite un grant temporal con
`context->issue_link_grant()` y lo muestra al usuario (por ejemplo, un QR); no
se debe publicar un endpoint HTTP que emita grants. La página web coloca
`Session::csrf_token()` en un campo oculto de su formulario POST. El handler
verifica ese token antes de canjear el grant contra el ID de sesión HTTP ya
persistido:

```cpp
const auto submitted_csrf = request.input("_token");
if (!submitted_csrf ||
    !kotavel::http::Session::verify_csrf_token(*submitted_csrf)) {
    return Response{403, "text/plain", "CSRF token rejected"};
}

const auto submitted_grant = request.input("grant");
if (!submitted_grant) return Response{403, "text/plain", "link rejected"};
const bool linked = resolver->redeem_link_grant(
    kotavel::http::SessionLinkGrant{std::string(*submitted_grant)},
    kotavel::http::Session::id());
```

El token CSRF se guarda en la sesión del servidor y no aparece mediante
`Session::all()`, `get()`, `has()` ni `exists()`. `Session::regenerate()` rota
el token para evitar reutilizarlo tras cambiar el identificador.

Los grants expiran por defecto en 60 segundos, tienen un máximo de 10 minutos
y se consumen dentro de una transacción atómica; solicitudes concurrentes solo
pueden canjearlo una vez. La base de datos restringe los permisos al usuario del
proceso. No se usan IP, `localhost` ni cabeceras como prueba de autorización.
La sesión debe existir antes del canje, y regenerar, invalidar o eliminar una
sesión revoca su vínculo. `ContextId` de otra instalación no existe en la base
local y no puede emitir un grant válido.

Los valores del contexto pueden tener TTL individual (máximo 30 días); al vencer,
`get()` deja de exponerlos aunque la limpieza física aún no haya ocurrido. Para
acotar recursos, cada valor acepta hasta 256 KiB, cada contexto hasta 1.024 claves
y 4 MiB, una instalación hasta 4.096 contextos y cada contexto hasta 32 grants
activos.
El contexto en sí permanece hasta que la aplicación lo retire mediante una
política de ciclo de vida propia, llamando `Context::revoke()` o
`ContextResolver::delete_context()`; esa operación también revoca grants y
vínculos asociados.

La recolección periódica de sesiones expiradas se agenda desde el middleware y se
ejecuta en un trabajador de fondo. Se activa por tiempo o cada 100 solicitudes,
agrupa solicitudes concurrentes mientras hay una recolección en curso y no bloquea
la respuesta HTTP; un error de mantenimiento no falla la petición y permite
intentar una recolección posterior.

SQLite mantiene índices por vencimiento para sesiones, grants y valores del
contexto, además de un índice `(context_id, expires_at)` para contar grants
activos sin recorrer la tabla completa. La prueba de recuperación verifica con
`EXPLAIN QUERY PLAN` que las consultas de expiración usan esos índices.

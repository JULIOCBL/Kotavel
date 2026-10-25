# HTTP

`http` contiene un kernel independiente del servidor: router, request/response, middleware, validación tipo FormRequest, reporte de excepciones y puente de sesión. Axum u otro servidor adapta tráfico de red a estos tipos.

## Ejemplo de rutas

```rust
use kotavel::http::{ApiResponse, Route};

let routes = kernel.routes_mut()?;
Route::api(routes)
    .get("/health", Arc::new(|_| ApiResponse::success(&"ok").unwrap()))?
    .name("api.health")?;
```

Registra middleware global, por grupo o alias antes de despachar. Mantén los handlers como adaptadores delgados; la lógica de negocio y persistencia pertenecen a servicios de aplicación.

## Mapa de la API HTTP

El módulo reúne cinco áreas; las rutas HTTP completas están en la
[guía de rutas del Laboratorio](../../../Laboratory-Kotavel/docs/modules/routes.md).

| Área | Qué ofrece |
| --- | --- |
| `Router` / `Route` | API, web, cinco verbos, prefijos, grupos, nombres y middleware |
| `Request` | método, path, body, query, headers, parámetros, inputs, arrays, objetos, archivos y sesión |
| `Response` / `ApiResponse` | respuestas propias, envelopes JSON de éxito/error y códigos HTTP |
| `FormRequest` | autorización, reglas, valores validados y respuesta 400/403/422 |
| `HttpKernel` / `Server` | middleware global/grupal/ruta, reporte de excepciones y adaptadores de servidor |

El request expone `input`, `query`, `route`, `header`, `file`, `all`,
`exists`, `has`, `filled`, `is_null`, `is_array` e `is_object`.
También da acceso a una sesión instalada por middleware y al contexto de
sesión enlazado. La lista completa de métodos públicos se documenta en las
secciones API de las guías de rutas, requests y sesiones.

Las reglas FormRequest disponibles incluyen required/nullable/sometimes,
tipos, formato y rango, reglas condicionales, exclusión de campos, arrays y
objetos, archivos/imágenes, fechas, URLs, IP, regex, comparación y contraseña
actual. `bail` corta validaciones restantes de un campo. Las reglas no
reconocidas producen `UnsupportedRule`, no se ignoran en silencio.

## Referencia de métodos HTTP

### Request

| Método | Uso |
| --- | --- |
| `method()`, `kind()`, `path()`, `raw_method()`, `body()`, `query_string()` | Inspeccionar el transporte y la ruta ya normalizada |
| `input(key)`, `query(key)`, `route(key)`, `header(key)` | Leer campos según su origen |
| `has_valid_input()` | Comprobar que el cuerpo pudo interpretarse |
| `file(key)` | Obtener un archivo subido (nombre, MIME y bytes) |
| `exists/has/is_null/is_array/is_object/filled(key)` | Consultar presencia y forma de un campo |
| `array_values/array_keys(key)`, `all()` | Leer listas, objetos y valores aplanados |
| `session()`, `session_context()`, `redeem_session_context_link(grant)` | Acceder a sesión y contexto compartido autorizado |

El objeto `Session` devuelto por `Request::session()` ofrece `id`, `get`,
`get_value`, `has`, `exists`, `all`, `put`, `forget`, `increment`, `flush`,
`csrf_token`, `verify_csrf_token`, `regenerate` e `invalidate`. El acceso
devuelve un error `Unavailable` cuando la ruta no instaló middleware de
sesión.

### FormRequest

Implementa `FormRequest::authorize(request)` y `rules()`; se puede personalizar
`current_password_matches(password, guard)`. Ejecuta la validación con
`validate_form(request, &form)` o adapta la lógica mediante `form_handler(build,
action)`. `FormValidation` expone `is_valid()`, `errors()`, `validated()` y
`error_response()`. El handler final solo se ejecuta después de autorizar y
validar; los fallos producen respuestas 403, 400 o 422. La tabla de reglas y
argumentos está en [Requests del Laboratorio](../../../Laboratory-Kotavel/docs/modules/requests.md).

### Responses

| API | Resultado |
| --- | --- |
| `Response::new(status, content_type, body)` | Respuesta HTTP propia |
| `ApiResponse::success(data)` | Envelope JSON de éxito |
| `success_with(data, status, title, message)` | Éxito con metadatos |
| `error(status, errors)`, `single_error(status, title, detail)` | Envelope JSON de error |

Los códigos de éxito deben ser 2xx y los de error 4xx/5xx. `Response` permite
añadir headers; el adaptador HTTP los transfiere al transporte.

### HttpKernel y Server

`HttpKernel` expone `new`, `routes/routes_mut`, `use_global`, `use_api`,
`use_web`, `alias_middleware`, `configure_middleware`,
`set_exception_reporter`, `seal/started`, `handle`, `not_found`,
`method_not_allowed`, `run` y `stop`. Antes de arrancar se pueden configurar
rutas y middleware. `run` valida `ServerSettings`, sella el kernel y entrega
una instantánea inmutable al servidor.

`ServerSettings::validate()` y `socket_address()` verifican host, puerto y
resolución. Un adaptador implementa `Server::run(kernel, settings, started)` y
`stop()`; el núcleo no abre sockets por cuenta propia. El middleware de sesión
se instala con `start_session(manager, transport)` o
`start_session_with_context(manager, transport, resolver)`.

# Rutas HTTP y deep links

Kotavel separa el registro de rutas de la biblioteca que escucha sockets. El
target opcional `Kotavel::Http` implementa actualmente el contrato con Drogon;
`Kotavel::Core` no depende de él. API y web comparten un listener HTTP, pero sus
rutas se registran como grupos distintos.

## Respuestas JSON de API

`kotavel::http::ApiResponse` es parte de la API pública en
`<kotavel/http/api_response.hpp>`. Los handlers API pueden usar `success()` para
convertir vectores, mapas y valores compatibles a JSON. Las respuestas siguen
este formato:

```cpp
Route.get("/users", [](const kotavel::http::Request&) {
    const std::vector<User> users = load_users();
    return kotavel::http::ApiResponse::success(users, 200, "Users", "Users loaded");
});
```

```json
{"data":[{"id":1,"name":"Ada"}],"meta":{"title":"Users","message":"Users loaded"}}
```

### Negociación del formato de respuesta

El Kernel usa `Accept` para comprobar si el cliente puede recibir el formato
predeterminado de la ruta. Las rutas API ofrecen `application/json`; las rutas
web ofrecen `text/html`. Si falta `Accept`, se usa el formato de la ruta. También
se aceptan rangos como `application/*`, `text/*` y `*/*`, junto con sus valores
de calidad `q`. Si el cliente excluye el formato de la ruta o solo acepta otro,
Kotavel responde `406 Not Acceptable` sin ejecutar middleware ni el handler.
Los errores siguen el formato predeterminado de la ruta: JSON en API y HTML en
web. Cada respuesta incluye el `Content-Type` de su representación.

`Content-Type` tiene otro propósito: describe el formato del cuerpo recibido,
por ejemplo `application/json` en un POST con JSON.

Los tipos propios se convierten mediante una especialización pública de
`kotavel::http::ApiJsonSerializer<T>`. Las claves de `std::map` y
`std::unordered_map` deben ser cadenas o enteros; las claves enteras se
convierten en cadenas JSON. El orden de `std::unordered_map` no está definido.

```cpp
template <>
struct kotavel::http::ApiJsonSerializer<User> {
    static kotavel::http::JsonValue serialize(const User& user) {
        return kotavel::http::JsonValue::Object{
            {"id", user.id},
            {"name", user.name},
        };
    }
};
```

`error()` acepta una lista para errores de validación o un solo título y detalle:

```cpp
return kotavel::http::ApiResponse::error(422, {
    {"Validation Failed", "The email is required", std::string("email")},
    {"Validation Failed", "The name is required", std::string("name")},
});
```

Esto produce un objeto `errors` con el estado HTTP, título, detalle y, cuando
existe, `source.field`. El Kernel usa el mismo formato para errores API 404 y
500. Las rutas web siguen devolviendo HTML; el Kernel presenta esos errores en
HTML también. `ApiResponse::success(data, 204)` devuelve una respuesta vacía.

RapidJSON se usa internamente para escribir la respuesta. Los serializadores
personalizados usan `JsonValue`, el contrato JSON propio de Kotavel, para que la
biblioteca interna se pueda sustituir sin cambiar el código de las aplicaciones.

## Rutas API y web

```cpp
kotavel::http::HttpKernel kernel(kotavel::http::make_server());
auto Route = kotavel::http::Route::api(kernel.routes());

Route.get("/health", [](const kotavel::http::Request&) {
    return kotavel::http::Response{
        200, "application/json; charset=utf-8", R"({"status":"ok"})"};
}).name("api.health");

auto Web = kotavel::http::Route::web(kernel.routes());
Web.get("/", [](const kotavel::http::Request&) {
    return kotavel::http::Response{
        200, "text/html; charset=utf-8", "<h1>Inicio</h1>"};
});

const auto health_url = kernel.routes().url_for("api.health"); // "/api/health"
kernel.run();
```

`Route::api(router)` y `Route::web(router)` crean contextos explícitos por grupo.
En ellos se usan `get`, `post`, `put`, `patch` y `remove` (`remove` registra
el verbo HTTP DELETE y evita usar la palabra reservada `delete` de C++). El grupo API añade
el prefijo `/api`; web conserva su ruta. A cada endpoint se le puede asignar un
nombre único con `.name("api.health")`; `Router::url_for("api.health")` devuelve
la URL registrada. Los nombres son opcionales. Los parámetros dinámicos se leen
desde `request.route("id")`. `Request` ofrece `input()`, `query()`, `route()`,
`header()`, `all()`, `has()` y `filled()`. `input()` consulta primero el cuerpo y
después la query; `query()` solo consulta la URL. El helper `request()` permite
acceder a la solicitud activa desde middleware y handlers. Para agrupar por prefijo:

```cpp
Route.prefix("users").group([](auto& Route) {
    Route.get("/", list_users);
    Route.post("/", create_user);
});
```

## Kernel y middleware

La referencia completa, con el contrato, orden, aliases, grupos y límites del
módulo está en [Middleware HTTP](http/middleware.md).

`HttpKernel` ejecuta la cadena antes de entregar la respuesta al transporte.
El Kernel acepta una configuración declarativa ordenada como el Kernel de
Laravel: middleware global, defaults de los grupos API/web y el registro
`route_middleware` de aliases. Cada middleware se implementa como callback C++;
las rutas usan su alias como cadena:

```cpp
kernel.configure_middleware({
    .middleware = { /* middleware global */ },
    .middleware_groups = {
        .api = { /* defaults API */ },
        .web = { /* defaults web */ },
    },
    .route_middleware = {
        {"ensure.auth", authenticate},
        {"company.billing", check_billing},
        {"company.context", load_company_context},
    },
});

auto Admin = Route.prefix("admin")
    .middleware({"ensure.auth", "company.billing"});
Admin.get("/users", list_users);

Route.get("/private", private_handler)
    .middleware("company.context")
    .name("api.private");
```

Una cadena asigna un alias a una ruta; una lista asigna varios y conserva el
orden escrito. Antes de llamar `run()`, Kotavel valida cada alias al configurar
la ruta. Un alias desconocido, vacío o duplicado genera un error de
configuración, en vez de fallar durante una petición. El registro se congela al
arrancar. Para configuración incremental también están disponibles
`use_global()`, `use_api()`, `use_web()` y `alias_middleware()`.

También es posible registrar middleware inline para casos puntuales:

```cpp
kernel.use_global([](const auto& request, const kotavel::http::Next& next) {
    // Un middleware puede responder directamente para rechazar una solicitud.
        if (request.path() == "/maintenance")
        return kotavel::http::Response{503, "text/plain", "Mantenimiento"};
    return next();
});

// Middleware heredado por todas las rutas del grupo /maintenance.
auto Maintenance = Route.prefix("maintenance").middleware(
    [](const auto& request, const kotavel::http::Next& next) {
        if (request.body() == "deny")
            return kotavel::http::Response{403, "text/plain", "Prohibido"};
        return next();
    });
Maintenance.get("/status", maintenance_status);

// Los grupos también se pueden aplicar por tipo con use_api() o use_web().
kernel.use_api(api_defaults);
kernel.use_web(web_defaults);
```

El orden es middleware global, grupo (`use_api` o `use_web`), middleware
asociado a la ruta y handler. Cada middleware puede llamar `next()` una vez o
devolver una respuesta para detener la cadena. Llama `next()` sincrónicamente y
no conserves ese callback después de retornar. Si el middleware o handler lanza
una excepción, el Kernel devuelve un 500 genérico. Si el consumidor configura
un `ExceptionReporter`, el Kernel también entrega la ruta, la solicitud y la
excepción original al callback; el callback se puede ejecutar en paralelo y sus
fallos no alteran la respuesta HTTP. Los middleware son callbacks síncronos;
deben ser seguros para concurrencia y no bloquear el event loop.
Antes de registrar el verbo, `.middleware(...)` configura el contexto/grupo; al
encadenarlo después de `get(...)`, `post(...)` u otro verbo afecta únicamente a
ese endpoint. Los aliases se resuelven durante la configuración, no en cada
solicitud. Todavía no hay parámetros de alias, grupos
nombrados personalizados, sesiones, CSRF ni limitador de solicitudes incluidos.

Para validar antes de llegar al controlador, deriva una clase de `FormRequest`,
define `authorize()` y `rules()`, y enlaza el controlador al contexto de rutas.
Kotavel valida antes de invocar la acción; `validated()` devuelve solo los campos
declarados en las reglas. Las peticiones API inválidas usan el error JSON estándar
con estado 422; la autorización denegada devuelve 403.

```cpp
class StoreItemRequest : public kotavel::http::FormRequest {
public:
    using FormRequest::FormRequest;
    kotavel::http::ValidationRules rules() const override {
        return {{"name", {"required", "string", "min:3"}}};
    }
};

class ItemController : public kotavel::http::Controller {
public:
    kotavel::http::Response store(const StoreItemRequest& form) const {
        return kotavel::http::ApiResponse::success(form.validated(), 201);
    }
};

auto items = kotavel::http::Route::api(routes).prefix("items").controller(controller);
items.post("/", &ItemController::store);
```

`ValidationRules` expone reglas Laravel como cadenas. Incluye presencia y
condiciones (`required`, `present`, `missing`, `filled`, `sometimes`, `nullable`,
`required_if`, `required_if_accepted`, `required_if_declined`, `required_with`,
`accepted_if`, `declined_if`, `prohibited_if`, `exclude_if` y sus variantes),
tipos y texto (`string`, `array`, `list`, `integer`, `numeric`, `boolean`,
`accepted`, `declined`, `email`, `url`, `active_url`, `ip`, `ipv4`, `ipv6`,
`json`, `alpha`, `alpha_num`, `alpha_dash`, `ascii`, `lowercase`, `uppercase`,
`regex`, `not_regex`, `in`, `not_in`, `enum`, `same`, `different`, `confirmed`,
`current_password`, `starts_with`, `ends_with`, `doesnt_start_with`,
`doesnt_end_with`, `hex_color`, `mac_address`, `uuid` y `ulid`), límites (`min`, `max`,
`size`, `between`, `decimal`, `digits`, `digits_between`, `min_digits`,
`max_digits`, `gt`, `gte`, `lt`, `lte`, `multiple_of`), fechas, arreglos y
archivos (`after`, `before`, `date_format`, `timezone`, `distinct`, `contains`,
`in_array`, `array_keys`, `required_array_keys`, `file`, `image`, `mimes`,
`mimetypes`, `extensions`, `dimensions`, `encoding`) y las demás reglas auxiliares
de Laravel que aceptan este contrato. Las reglas de base de datos `exists` y
`unique` requieren un proveedor de datos y no se incluyen.

El DSL recibe reglas como `vector<string>`; no puede recibir los objetos fluent de
Laravel como `Password`, `Rule::anyOf` o reglas personalizadas. Para esas
validaciones, expresa las reglas simples en la lista o deriva `FormRequest` y
sobrescribe `validate()`/`authorize()` según el contrato público. `current_password`
llama al proveedor de autenticación de la aplicación mediante
`current_password_matches(password, guard)`. Sin proveedor, la regla falla. La
verificación `email:dns` y `active_url` requiere resolución DNS disponible en el
sistema. `email:dns` comprueba que el host resuelva; `email:spoof` requiere ICU y
no está disponible en este adaptador. `date_format` cubre los tokens PHP
habituales (`Y`, `y`, `m`, `n`, `d`,
`j`, `H`, `h`, `G`, `g`, `i`, `s`) y `timezone` consulta la base IANA instalada.

El DSL usa callbacks C++ y respuestas explícitas, sin reflexión ni resolución
global de fachadas. Un mismo par método/ruta y cada nombre asignado deben ser
únicos. Los handlers son síncronos: no deben bloquear el event loop. `Request`
posee los datos de transporte; el helper `request()` devuelve una referencia
válida solo durante la ejecución del middleware o handler. Copia los valores antes
de conservarlos. El
registro se congela al arrancar el kernel, que es de un solo uso por proceso.

El adaptador puede procesar handlers simultáneamente. Los handlers y servicios
que capturen deben ser seguros para concurrencia. El contenedor actual no permite
resolver ni registrar servicios concurrentemente: resuelve dependencias antes de
arrancar el servidor y captura referencias de servicios ya construidos.

El Laboratorio conecta `ExceptionReporter` con `Application::log()`: las
excepciones de rutas y middleware se escriben como `error` con el código
`http.unhandled_exception` en el canal configurado (`storage/logs/kotavel.log`
por defecto). El registro incluye método, ruta declarada y motivo; el cliente
solo recibe una respuesta HTTP 500 genérica. Si el logger falla, la respuesta
se conserva y el fallo del logger no escapa del Kernel. El módulo aún no implementa parámetros
tipados de rutas, ámbitos de solicitud, cancelación ni conexión
automática con `Application::execute()`. Esos límites deben completarse antes de
considerarlo listo para producción.

Para conectar otro consumidor al logger del núcleo, configura el reporter antes
de iniciar el servidor:

```cpp
kernel.set_exception_reporter([&application](const auto& route, const auto& request,
                                             std::exception_ptr failure) {
    // El callback puede ejecutarse desde los hilos del adaptador HTTP.
    // Extrae el motivo y envíalo a application.log() con datos de la ruta.
});
```

## Registro de peticiones

El adaptador imprime una línea por cada petición HTTP en la terminal donde se
ejecuta la aplicación, incluidas respuestas automáticas como 404 y 405. Incluye
fecha y hora UTC, método, ruta solicitada, estado HTTP y duración hasta enviar
la respuesta:

```text
[HTTP] 2026-10-17T18:42:03.127Z GET /api/health 200 0.184 ms
```

La duración usa un reloj monotónico y cubre la ejecución de middleware y el
handler; no mide el viaje completo por la red. El registro no incluye query
strings ni cuerpos. Las líneas se sincronizan para que las peticiones
simultáneas no mezclen su salida.

## Listener y acceso de red

El valor predeterminado es `0.0.0.0:8080`, para aceptar conexiones IPv4 en las
interfaces disponibles. Configura `HTTP_HOST=127.0.0.1` para aceptar solo
conexiones desde la misma máquina. El puerto debe estar entre 1 y 65535.
`0.0.0.0` es la dirección de escucha, no la dirección del cliente: otro equipo
debe conectarse a la IP LAN del servidor. El firewall y la red deben permitir
ese tráfico.

## Navegación desktop

Las rutas desktop y sus deep links pertenecen al módulo
[desktop](desktop.md), que integra el registro de destinos, la validación de la
URI y el despacho de la ventana en un solo router.

## Dependencia opcional

En el proyecto consumidor activa el módulo en `kotavel.json`:

```json
{
  "mode": "local",
  "source_dir": "../Kotavel",
  "version": "0.1.0",
  "features": { "http": true }
}
```

`python3 kotavel.py install` añade Drogon al grafo Conan solo cuando `http` está
activado; `python3 kotavel.py build` compila el módulo. Drogon queda detrás de
`Kotavel::Http`, de modo que la app usa `Router`, `Response` y `HttpKernel`, no
sus tipos. El adaptador podrá cambiarse sin cambiar las rutas del consumidor.

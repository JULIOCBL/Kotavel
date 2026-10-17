# Rutas HTTP y deep links

Kotavel separa el registro de rutas de la biblioteca que escucha sockets. El
target opcional `Kotavel::Http` implementa actualmente el contrato con Drogon;
`Kotavel::Core` no depende de él. API y web comparten un listener HTTP, pero sus
rutas se registran como grupos distintos.

## Rutas API y web

```cpp
kotavel::http::HttpKernel kernel(kotavel::http::make_server());
auto Route = kotavel::http::Route::api(kernel.routes());

Route.get("/health", [](const kotavel::http::RequestView&) {
    return kotavel::http::Response{
        200, "application/json; charset=utf-8", R"({"status":"ok"})"};
}).name("api.health");

auto Web = kotavel::http::Route::web(kernel.routes());
Web.get("/", [](const kotavel::http::RequestView&) {
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
la URL registrada. Los nombres son opcionales; los parámetros dinámicos y la
generación de URL con parámetros aún no están implementados. Para agrupar por
prefijo:

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
    if (request.path == "/maintenance")
        return kotavel::http::Response{503, "text/plain", "Mantenimiento"};
    return next();
});

// Middleware heredado por todas las rutas del grupo /maintenance.
auto Maintenance = Route.prefix("maintenance").middleware(
    [](const auto& request, const kotavel::http::Next& next) {
        if (request.body == "deny")
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

El DSL usa callbacks C++ y respuestas explícitas, sin reflexión ni resolución
global de fachadas. Un mismo par método/ruta y cada nombre asignado deben ser
únicos. Los handlers son síncronos: no deben bloquear el event loop. Las vistas de `RequestView` solo
son válidas durante la llamada; copia los datos antes de conservarlos. El
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

El Kernel imprime una línea por handler en la terminal donde se ejecuta la
aplicación. Incluye fecha y hora UTC, método, ruta solicitada, estado HTTP y
duración de toda la cadena de middleware y el handler:

```text
[HTTP] 2026-10-17T18:42:03.127Z GET /api/health 200 0.184 ms
```

La duración usa un reloj monotónico y cubre la ejecución de middleware y el
handler hasta que se construye la respuesta; no mide el viaje completo por la red. El
registro no incluye query strings ni cuerpos. Las líneas se sincronizan para que
las peticiones simultáneas no mezclen su salida.

## Listener y acceso de red

El valor predeterminado es `0.0.0.0:8080`, para aceptar conexiones IPv4 en las
interfaces disponibles. Configura `HTTP_HOST=127.0.0.1` para aceptar solo
conexiones desde la misma máquina. El puerto debe estar entre 1 y 65535.
`0.0.0.0` es la dirección de escucha, no la dirección del cliente: otro equipo
debe conectarse a la IP LAN del servidor. El firewall y la red deben permitir
ese tráfico.

## Deep links de escritorio

Un deep link es una URI que el sistema operativo entrega a la aplicación de
escritorio. Kotavel valida el esquema y traduce el destino a una pantalla que la
aplicación registró explícitamente:

```cpp
kotavel::desktop::DeepLinkRouter links("kotavel");
links.screen("pos/home", [] {
    // La aplicación activa la Screen de inicio del punto de venta.
});
links.dispatch("kotavel://pos/home");
```

Solo se invocan callbacks registrados; no se aceptan destinos arbitrarios. API
y web usan URLs HTTP y no son deep links de escritorio. El despacho está
implementado; la asociación del esquema con el ejecutable aún depende del
instalador o de la configuración específica de Windows, macOS y Linux. La app
debe entregar al router las URIs recibidas al iniciar o activarse.

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

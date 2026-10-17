# Middleware HTTP

Esta guía documenta el contrato de middleware de `Kotavel::Http`. La
configuración pertenece al consumidor; el núcleo ejecuta los callbacks en un
orden definido y mantiene las rutas independientes del adaptador de transporte.

## Modelo

La estructura de `MiddlewareConfiguration` está organizada como el Kernel de
Laravel:

| Kotavel | Equivalente conceptual de Laravel | Cuándo se ejecuta |
| --- | --- | --- |
| `middleware` | `$middleware` | En cada solicitud, antes del grupo |
| `middleware_groups.api` | `$middlewareGroups['api']` | En cada ruta API |
| `middleware_groups.web` | `$middlewareGroups['web']` | En cada ruta web |
| `route_middleware` | `$routeMiddleware` | Cuando una ruta o grupo selecciona su alias |

Los nombres `api` y `web` son los grupos incluidos actualmente. El bloque
completo se registra antes de definir rutas:

```cpp
kernel.configure_middleware({
    .middleware = {
        global_middleware,
    },
    .middleware_groups = {
        .api = {api_defaults},
        .web = {web_defaults},
    },
    .route_middleware = {
        {"auth", authenticate},
        {"company.context", load_company_context},
    },
});
```

Las listas pueden estar vacías. No agregues middleware global o de grupo si la
aplicación no lo necesita. También existen `use_global()`, `use_api()`,
`use_web()` y `alias_middleware()` para configurar incrementalmente el Kernel.

## Contrato de un middleware

El tipo base es:

```cpp
using Middleware = std::function<Response(const RequestView&, const Next&)>;
using Next = std::function<Response()>;
```

El callback recibe una vista de la solicitud y la continuación. Puede inspeccionar
la solicitud, ejecutar `next()` y modificar la respuesta, o devolver una
respuesta directamente para detener el resto de la cadena. El tipo actual es
síncrono: llama `next()` durante la ejecución del middleware, como máximo una vez,
y no guardes la referencia para usarla luego.

```cpp
kotavel::http::Middleware require_body = [](const auto& request, const auto& next) {
    if (request.body.empty())
        return kotavel::http::Response{400, "text/plain; charset=utf-8", "Body requerido"};
    return next();
};
```

El orden es global → grupo API/web → grupo explícito de ruta → endpoint → handler.
Al retornar, las respuestas vuelven en orden inverso por los middleware que
llamaron a `next()`.

## Alias de `route_middleware`

Cada par de `route_middleware` tiene una clave textual y un callback. Los aliases
se validan y resuelven al registrar rutas; no hay una búsqueda del nombre por
cada solicitud. Las claves deben ser no vacías y únicas. Una ruta que use un
alias no registrado falla durante la configuración, antes de arrancar el
servidor.

Una ruta puede usar un alias:

```cpp
Route.post("/private", save_record)
    .middleware("auth")
    .name("api.records.store");
```

O varios, en el orden declarado:

```cpp
Route.post("/private", save_record)
    .middleware({"auth", "company.context"});
```

También puedes aplicar aliases a un grupo completo:

```cpp
Route.prefix("company")
    .middleware({"auth", "company.context"})
    .group([](auto& Route) {
        Route.get("/profile", show_profile);
        Route.patch("/profile", update_profile);
    });
```

El middleware del grupo se copia a sus rutas hijas. Un alias encadenado después
del método HTTP se aplica solo al endpoint recién registrado. Antes de un
método, se hereda por las rutas que se registren desde ese contexto.

## Crear una clase middleware

La aplicación puede tener una clase por middleware, con un método `handle()`
similar al de Laravel. Kotavel recibe un callback, así que la clase expone un
adaptador que captura una copia del objeto:

```cpp
class RequireToken final {
public:
    kotavel::http::Response handle(
        const kotavel::http::RequestView& request,
        const kotavel::http::Next& next) const;

    kotavel::http::Middleware as_middleware() const;
};
```

```cpp
kotavel::http::Middleware RequireToken::as_middleware() const {
    return [middleware = *this](const auto& request, const auto& next) {
        return middleware.handle(request, next);
    };
}
```

Registra una instancia con su nombre en `route_middleware` y aplica el nombre
desde los archivos de rutas. Mantener la clase en la aplicación evita que las
reglas específicas del consumidor entren al núcleo.

## Validación y errores

- Se rechazan callbacks vacíos en las pilas global, API, web y aliases.
- Se rechazan aliases vacíos, duplicados o desconocidos.
- Un middleware puede cortar el pipeline devolviendo una `Response` sin llamar
  `next()`.
- Las excepciones de middleware y handlers se convierten en una respuesta HTTP
  500 genérica; el texto interno de la excepción no se entrega al cliente.
- Al arrancar el `HttpKernel`, rutas, defaults y aliases quedan sellados.

La estructura `RequestView` contiene vistas no propietarias. No conserves
`path`, `body` ni el objeto de continuación después de regresar del callback.
El adaptador puede atender peticiones simultáneamente; las clases middleware no
deben compartir estado mutable sin sincronización. Evita bloquear el event loop.

## Límites actuales

Los grupos predeterminados son `api` y `web`; todavía no se pueden definir
grupos con nombres arbitrarios ni parámetros como `throttle:60,1`. `Response`
incluye estado, tipo de contenido y cuerpo, pero no un mapa general de
encabezados. Por ello, un `SecurityHeadersMiddleware` como el ejemplo de Laravel
requiere primero ampliar el contrato de respuesta y su adaptación al servidor.

## Pruebas

La suite `kotavel.http.middleware` cubre la configuración declarativa, el orden
y aislamiento de grupos, aliases individuales y en listas, validación, cortes
de cadena, excepciones y sellado al iniciar. Usa un adaptador falso:

```sh
ctest --test-dir build/core -C Debug -R '^kotavel\.http\.middleware$' --output-on-failure
```

Consulta la [guía práctica del Laboratorio](../../../Laboratory-Kotavel/docs/middleware.md)
para ver los archivos de una aplicación consumidora y probar las respuestas HTTP.

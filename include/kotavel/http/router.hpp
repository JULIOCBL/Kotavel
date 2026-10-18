#pragma once

#include <cstddef>
#include <functional>
#include <initializer_list>
#include <string>
#include <string_view>
#include <unordered_map>
#include <vector>

#include <kotavel/http/request.hpp>

namespace kotavel::http {

struct Response {
    int status = 200;
    std::string content_type = "text/plain; charset=utf-8";
    std::string body;
};

using Handler = std::function<Response(const Request&)>;
using Next = std::function<Response()>;
using Middleware = std::function<Response(const Request&, const Next&)>;

struct RouteDefinition final {
    Method method;
    RouteKind kind;
    std::string path;
    std::string name;
    Handler handler;
    std::vector<Middleware> middleware;
};

class Router final {
public:
    // Las rutas API reciben el prefijo /api para mantener clara su frontera.
    void api_get(std::string_view path, Handler handler);
    void api_post(std::string_view path, Handler handler);
    void api_put(std::string_view path, Handler handler);
    void api_patch(std::string_view path, Handler handler);
    void api_delete(std::string_view path, Handler handler);
    void web_get(std::string_view path, Handler handler);
    void web_post(std::string_view path, Handler handler);
    void web_put(std::string_view path, Handler handler);
    void web_patch(std::string_view path, Handler handler);
    void web_delete(std::string_view path, Handler handler);

    // El servidor lee la tabla inmutable después de registrar todas las rutas.
    [[nodiscard]] const std::vector<RouteDefinition>& routes() const noexcept { return routes_; }
    // Devuelve la URL registrada para un nombre único de ruta.
    [[nodiscard]] std::string url_for(std::string_view name) const;
    // Registra un alias para que las rutas puedan declarar middleware por nombre.
    void alias_middleware(std::string_view alias, Middleware middleware);
    void seal() noexcept { sealed_ = true; }
    [[nodiscard]] bool sealed() const noexcept { return sealed_; }

private:
    friend class Route;
    void add(RouteKind kind, Method method, std::string_view path, Handler handler,
             std::vector<Middleware> middleware = {});
    void set_name(std::size_t index, std::string_view name);
    void add_middleware(std::size_t index, Middleware middleware);
    [[nodiscard]] Middleware middleware_for(std::string_view alias) const;
    std::vector<RouteDefinition> routes_;
    std::unordered_map<std::string, std::size_t> named_routes_;
    std::unordered_map<std::string, Middleware> middleware_aliases_;
    bool sealed_ = false;
};

// DSL con nombres cercanos a Laravel y contexto explícito para evitar estado global.
template <typename ControllerType>
class ControllerRoutes;

class Route final {
public:
    using Group = std::function<void(Route&)>;

    [[nodiscard]] static Route api(Router& router);
    [[nodiscard]] static Route web(Router& router);

    Route get(std::string_view path, Handler handler) const;
    Route post(std::string_view path, Handler handler) const;
    Route put(std::string_view path, Handler handler) const;
    Route patch(std::string_view path, Handler handler) const;
    // `remove` registra el verbo HTTP DELETE sin usar una palabra reservada de C++.
    Route remove(std::string_view path, Handler handler) const;

    // Nombra la ruta registrada inmediatamente antes, como ->name(...) en Laravel.
    Route name(std::string_view route_name) const;

    // Agrega middleware a las rutas del grupo sin modificar el contexto original.
    Route middleware(Middleware middleware) const;
    // Resuelve uno o varios aliases registrados previamente en el HttpKernel.
    Route middleware(std::string_view alias) const;
    Route middleware(std::initializer_list<std::string_view> aliases) const;

    // Agrupa rutas bajo un prefijo, como Route::prefix(...)->group(...) en Laravel.
    [[nodiscard]] Route prefix(std::string_view prefix) const;
    // Enlaza las siguientes acciones de esta ruta a una instancia de controlador.
    template <typename ControllerType>
    [[nodiscard]] ControllerRoutes<ControllerType> controller(ControllerType& instance) const;
    void group(const Group& routes) const;

private:
    Route(Router& router, RouteKind kind, std::string prefix = {});
    [[nodiscard]] Route add(Method method, std::string_view path, Handler handler) const;

    Router* router_;
    RouteKind kind_;
    std::string prefix_;
    std::vector<Middleware> middleware_;
    std::size_t last_route_index_ = static_cast<std::size_t>(-1);
};

} // namespace kotavel::http

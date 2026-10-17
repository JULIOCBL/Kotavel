#include <kotavel/http/router.hpp>

#include <stdexcept>
#include <utility>

namespace kotavel::http {
namespace {
std::string normalize_path(std::string_view path, RouteKind kind) {
    if (path.empty() || path.front() != '/' || path.find('?') != std::string_view::npos ||
        path.find('#') != std::string_view::npos)
        throw std::invalid_argument("Route path must be an absolute path without query or fragment");

    std::string result;
    if (kind == RouteKind::api) {
        if (path == "/api" || path.starts_with("/api/")) result.assign(path);
        else result = path == "/" ? "/api" : "/api" + std::string(path);
    } else {
        result.assign(path);
    }
    return result;
}
}

void Router::api_get(std::string_view path, Handler handler) {
    add(RouteKind::api, Method::get, path, std::move(handler));
}

void Router::api_post(std::string_view path, Handler handler) {
    add(RouteKind::api, Method::post, path, std::move(handler));
}

void Router::api_put(std::string_view path, Handler handler) {
    add(RouteKind::api, Method::put, path, std::move(handler));
}

void Router::api_patch(std::string_view path, Handler handler) {
    add(RouteKind::api, Method::patch, path, std::move(handler));
}

void Router::api_delete(std::string_view path, Handler handler) {
    add(RouteKind::api, Method::delete_, path, std::move(handler));
}

void Router::web_get(std::string_view path, Handler handler) {
    add(RouteKind::web, Method::get, path, std::move(handler));
}

void Router::web_post(std::string_view path, Handler handler) {
    add(RouteKind::web, Method::post, path, std::move(handler));
}

void Router::web_put(std::string_view path, Handler handler) {
    add(RouteKind::web, Method::put, path, std::move(handler));
}

void Router::web_patch(std::string_view path, Handler handler) {
    add(RouteKind::web, Method::patch, path, std::move(handler));
}

void Router::web_delete(std::string_view path, Handler handler) {
    add(RouteKind::web, Method::delete_, path, std::move(handler));
}

void Router::add(RouteKind kind, Method method, std::string_view path, Handler handler,
                 std::vector<Middleware> middleware) {
    if (sealed_) throw std::logic_error("HTTP routes are sealed after the kernel starts");
    if (!handler) throw std::invalid_argument("Route handler cannot be empty");
    auto normalized = normalize_path(path, kind);
    for (const auto& route : routes_) {
        if (route.method == method && route.path == normalized)
            throw std::logic_error("Duplicate HTTP route: " + normalized);
    }
    routes_.push_back(RouteDefinition{method, kind, std::move(normalized), {},
                                      std::move(handler), std::move(middleware)});
}

void Router::set_name(std::size_t index, std::string_view name) {
    if (sealed_) throw std::logic_error("HTTP routes are sealed after the kernel starts");
    if (index >= routes_.size()) throw std::logic_error("Route name requires a registered endpoint");
    if (name.empty()) throw std::invalid_argument("Route name cannot be empty");

    std::string new_name(name);
    const auto existing = named_routes_.find(new_name);
    if (existing != named_routes_.end() && existing->second != index)
        throw std::logic_error("Duplicate HTTP route name: " + new_name);
    const bool name_is_registered = existing != named_routes_.end();

    auto& route = routes_[index];
    if (route.name == new_name) return;

    // Añadir primero protege el nombre previo si la nueva asignación falla.
    if (!name_is_registered) {
        named_routes_.emplace(new_name, index);
    }
    if (!route.name.empty()) named_routes_.erase(route.name);
    route.name = std::move(new_name);
}

void Router::add_middleware(std::size_t index, Middleware middleware) {
    if (sealed_) throw std::logic_error("HTTP routes are sealed after the kernel starts");
    if (index >= routes_.size()) throw std::logic_error("Route middleware requires a registered endpoint");
    if (!middleware) throw std::invalid_argument("Route middleware cannot be empty");
    routes_[index].middleware.push_back(std::move(middleware));
}

std::string Router::url_for(std::string_view name) const {
    const auto named = named_routes_.find(std::string(name));
    if (named == named_routes_.end())
        throw std::out_of_range("Named HTTP route not found: " + std::string(name));
    return routes_[named->second].path;
}

void Router::alias_middleware(std::string_view alias, Middleware middleware) {
    if (sealed_) throw std::logic_error("HTTP middleware aliases are sealed after the kernel starts");
    if (alias.empty()) throw std::invalid_argument("HTTP middleware alias cannot be empty");
    if (!middleware) throw std::invalid_argument("HTTP middleware alias callback cannot be empty");

    const std::string key(alias);
    if (!middleware_aliases_.emplace(key, std::move(middleware)).second)
        throw std::logic_error("Duplicate HTTP middleware alias: " + key);
}

Middleware Router::middleware_for(std::string_view alias) const {
    const auto middleware = middleware_aliases_.find(std::string(alias));
    if (middleware == middleware_aliases_.end())
        throw std::invalid_argument("HTTP middleware alias not registered: " + std::string(alias));
    return middleware->second;
}

Route::Route(Router& router, RouteKind kind, std::string prefix)
    : router_(&router), kind_(kind), prefix_(std::move(prefix)) {}

Route Route::api(Router& router) { return Route(router, RouteKind::api); }
Route Route::web(Router& router) { return Route(router, RouteKind::web); }

Route Route::get(std::string_view path, Handler handler) const {
    return add(Method::get, path, std::move(handler));
}
Route Route::post(std::string_view path, Handler handler) const {
    return add(Method::post, path, std::move(handler));
}
Route Route::put(std::string_view path, Handler handler) const {
    return add(Method::put, path, std::move(handler));
}
Route Route::patch(std::string_view path, Handler handler) const {
    return add(Method::patch, path, std::move(handler));
}
Route Route::remove(std::string_view path, Handler handler) const {
    return add(Method::delete_, path, std::move(handler));
}

Route Route::name(std::string_view route_name) const {
    if (last_route_index_ == static_cast<std::size_t>(-1))
        throw std::logic_error("Route name must follow an endpoint registration");
    router_->set_name(last_route_index_, route_name);
    return *this;
}

Route Route::middleware(Middleware middleware) const {
    if (!middleware) throw std::invalid_argument("Route middleware cannot be empty");
    auto scoped_route = *this;
    if (last_route_index_ != static_cast<std::size_t>(-1)) {
        // Después de un verbo, Laravel-style middleware afecta solo ese endpoint.
        router_->add_middleware(last_route_index_, std::move(middleware));
        return scoped_route;
    }
    // Antes de un verbo, el middleware se hereda por el grupo o las rutas siguientes.
    scoped_route.middleware_.push_back(std::move(middleware));
    return scoped_route;
}

Route Route::middleware(std::string_view alias) const {
    return middleware(router_->middleware_for(alias));
}

Route Route::middleware(std::initializer_list<std::string_view> aliases) const {
    if (aliases.size() == 0)
        throw std::invalid_argument("HTTP middleware alias list cannot be empty");

    auto scoped_route = *this;
    for (const auto alias : aliases) scoped_route = scoped_route.middleware(alias);
    return scoped_route;
}

Route Route::prefix(std::string_view prefix) const {
    if (prefix.empty()) throw std::invalid_argument("Route prefix cannot be empty");
    std::string normalized(prefix);
    if (normalized.front() == '/') normalized.erase(0, 1);
    while (!normalized.empty() && normalized.back() == '/') normalized.pop_back();
    if (normalized.empty()) throw std::invalid_argument("Route prefix cannot be root");

    if (prefix_.empty()) {
        auto scoped_route = Route(*router_, kind_, std::move(normalized));
        scoped_route.middleware_ = middleware_;
        return scoped_route;
    }
    auto scoped_route = Route(*router_, kind_, prefix_ + "/" + normalized);
    scoped_route.middleware_ = middleware_;
    return scoped_route;
}

void Route::group(const Group& routes) const {
    if (!routes) throw std::invalid_argument("Route group callback cannot be empty");
    auto scoped_routes = *this;
    routes(scoped_routes);
}

Route Route::add(Method method, std::string_view path, Handler handler) const {
    std::string full_path;
    if (prefix_.empty()) {
        full_path.assign(path);
        if (full_path.empty() || full_path.front() != '/') full_path.insert(0, "/");
    } else {
        full_path = "/" + prefix_;
        if (!path.empty() && path != "/") {
            if (path.front() != '/') full_path.push_back('/');
            full_path.append(path);
        }
    }

    router_->add(kind_, method, full_path, std::move(handler), middleware_);
    auto registered_route = *this;
    registered_route.last_route_index_ = router_->routes_.size() - 1;
    return registered_route;
}

} // namespace kotavel::http

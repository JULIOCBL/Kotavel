#pragma once

#include <chrono>
#include <cstddef>
#include <cstdint>
#include <exception>
#include <functional>
#include <memory>
#include <string>
#include <string_view>
#include <utility>
#include <vector>

#include <kotavel/http/router.hpp>

namespace kotavel::http {

class HttpKernel;

struct ServerSettings {
    // 0.0.0.0 permite conexiones por interfaces locales y de otros equipos.
    std::string host = "0.0.0.0";
    std::uint16_t port = 8080;
    std::size_t threads = 0; // Cero deja que el adaptador elija su valor predeterminado.
};

using ServerStartedCallback = std::function<void()>;
// Permite integrar excepciones de solicitudes con el logger del consumidor.
using ExceptionReporter = std::function<void(const RouteDefinition&, const Request&,
                                             std::exception_ptr)>;

// Contrato del transporte; una implementación puede sustituirse sin tocar rutas.
class Server {
public:
    virtual ~Server() = default;
    virtual void run(const HttpKernel& kernel, const ServerSettings& settings,
                     const ServerStartedCallback& started) = 0;
    virtual void stop() noexcept = 0;
};

// Configuración declarativa del Kernel, organizada como middleware de Laravel.
struct MiddlewareGroups final {
    std::vector<Middleware> api;
    std::vector<Middleware> web;
};

struct MiddlewareConfiguration final {
    // Equivale a $middleware: se ejecuta para toda solicitud.
    std::vector<Middleware> middleware;
    // Equivale a $middlewareGroups: defaults compartidos por API y web.
    MiddlewareGroups middleware_groups;
    // Equivale a $routeMiddleware: alias que las rutas usan por cadena.
    std::vector<std::pair<std::string, Middleware>> route_middleware;
};

// Kernel HTTP ejecuta middleware y rutas; API y web comparten el listener.
class HttpKernel final {
public:
    explicit HttpKernel(std::unique_ptr<Server> server);
    ~HttpKernel();
    HttpKernel(const HttpKernel&) = delete;
    HttpKernel& operator=(const HttpKernel&) = delete;

    [[nodiscard]] Router& routes() noexcept { return routes_; }
    [[nodiscard]] const Router& routes() const noexcept { return routes_; }
    // Middleware global envuelve primero; luego corre el grupo API/web y el de la ruta.
    void use_global(Middleware middleware);
    void use_api(Middleware middleware);
    void use_web(Middleware middleware);
    // Alias disponibles para declarar middleware como cadenas en Route y grupos.
    void alias_middleware(std::string_view alias, Middleware middleware);
    // Aplica en un bloque las tres secciones equivalentes al Kernel de Laravel.
    void configure_middleware(MiddlewareConfiguration configuration);
    // El consumidor conecta el reporte antes de run(); el callback puede ejecutarse en varios hilos.
    void set_exception_reporter(ExceptionReporter reporter);
    [[nodiscard]] Response handle(const RouteDefinition& route,
                                  const Request& request) const;
    // Las solicitudes sin ruta registrada también generan una respuesta 404.
    [[nodiscard]] Response not_found(const Request& request) const;
    // Las rutas registradas con otro verbo generan 405 (403 para OPTIONS).
    [[nodiscard]] Response method_not_allowed(const Request& request) const;
    // El adaptador llama esto al enviar cada respuesta, incluidas las automáticas.
    void log_access(const Request& request, int status,
                    std::chrono::system_clock::time_point received_at,
                    std::chrono::steady_clock::duration elapsed) const noexcept;
    // Transport metadata is enough for logging; avoid parsing a second Request.
    void log_access(std::string_view method, std::string_view path, int status,
                    std::chrono::system_clock::time_point received_at,
                    std::chrono::steady_clock::duration elapsed) const noexcept;
    void run(const ServerSettings& settings = {},
             const ServerStartedCallback& started = {});
    void stop() noexcept;

private:
    std::unique_ptr<Server> server_;
    Router routes_;
    std::vector<Middleware> global_middleware_;
    std::vector<Middleware> api_middleware_;
    std::vector<Middleware> web_middleware_;
    ExceptionReporter exception_reporter_;
    bool started_ = false;
};

// Crea el adaptador HTTP configurado al compilar Kotavel::Http.
[[nodiscard]] std::unique_ptr<Server> make_server();

} // namespace kotavel::http

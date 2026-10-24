#pragma once

#include <filesystem>
#include <functional>
#include <memory>
#include <string>
#include <string_view>
#include <unordered_map>
#include <utility>
#include <vector>

#include <kotavel/session/context.hpp>

namespace kotavel::desktop {

class DesktopContext;

enum class DispatchResult { dispatched, invalid_path, not_found };

struct View final {
    // Identificador lógico, por ejemplo desktop.home.
    std::string name;
};

// Crea una referencia validada a una vista bajo resources/views/.
[[nodiscard]] View view(std::string_view name);

struct WindowSettings final {
    std::string title = "Kotavel";
    int width = 1100;
    int height = 720;
};

class WindowManager {
public:
    virtual ~WindowManager() = default;
    virtual void show(std::string_view id, const WindowSettings& settings,
                      const View& view) = 0;
    virtual void close(std::string_view id) noexcept = 0;
    [[nodiscard]] virtual bool is_visible(std::string_view id) const noexcept = 0;
    [[nodiscard]] virtual bool has_visible_window() const noexcept = 0;
};

using Handler = std::function<void(DesktopContext&)>;

class DesktopRouter final {
public:
    // Registra rutas absolutas; la ruta / es la pantalla inicial predeterminada.
    void get(std::string_view path, Handler handler);
    [[nodiscard]] DispatchResult dispatch(std::string_view path,
                                          DesktopContext& context) const;
    [[nodiscard]] bool contains(std::string_view path) const;

private:
    friend class Route;
    std::size_t add(std::string_view path, Handler handler);
    void set_name(std::size_t index, std::string_view name);

    struct Definition final {
        std::string path;
        std::string name;
        Handler handler;
    };
    std::vector<Definition> routes_;
    std::unordered_map<std::string, std::size_t> paths_;
    std::unordered_map<std::string, std::size_t> names_;
};

// DSL explícito del consumidor; los nombres web y desktop viven en routers distintos.
class Route final {
public:
    [[nodiscard]] static Route desktop(DesktopRouter& router);
    Route get(std::string_view path, Handler handler) const;
    Route name(std::string_view route_name) const;

private:
    explicit Route(DesktopRouter& router) : router_(&router) {}
    DesktopRouter* router_;
    std::size_t last_route_index_ = static_cast<std::size_t>(-1);
};

class DesktopPlatform {
public:
    using StartCallback = std::function<void(WindowManager&)>;
    using DeepLinkCallback = std::function<void(std::string_view, WindowManager&)>;
    virtual ~DesktopPlatform() = default;
    // Las plataformas que admiten enlaces mientras la aplicación ya corre pueden
    // conservar este callback hasta que inicie el loop gráfico.
    virtual void set_deep_link_handler(DeepLinkCallback) {}
    // La plataforma crea su loop gráfico y sus ventanas antes de ejecutar el arranque.
    virtual int run(int argc, char** argv, const std::filesystem::path& views_root,
                    const StartCallback& start) = 0;
};

class DesktopKernel final {
public:
    explicit DesktopKernel(std::unique_ptr<DesktopPlatform> platform,
                           std::filesystem::path views_root = "resources/views",
                           std::shared_ptr<kotavel::session::Context> session_context = {});
    DesktopKernel(const DesktopKernel&) = delete;
    DesktopKernel& operator=(const DesktopKernel&) = delete;

    [[nodiscard]] DesktopRouter& routes() noexcept { return routes_; }
    // Despacha / al iniciar si no se recibió un deep link como argumento.
    int run(int argc, char** argv, std::string_view scheme = "kotavel");

private:
    std::unique_ptr<DesktopPlatform> platform_;
    std::filesystem::path views_root_;
    DesktopRouter routes_;
    std::shared_ptr<kotavel::session::Context> session_context_;
};

// Un contexto de corta duración evita guardar acceso global al runtime gráfico.
class DesktopContext final {
public:
    DesktopContext(DesktopRouter& routes, WindowManager& windows,
                   std::shared_ptr<kotavel::session::Context> session_context = {})
        : routes_(&routes), windows_(&windows), session_context_(std::move(session_context)) {}

    void show(std::string_view window_id, const WindowSettings& settings,
              const View& view);
    void close(std::string_view window_id) noexcept;
    [[nodiscard]] bool is_visible(std::string_view window_id) const noexcept;
    [[nodiscard]] DispatchResult navigate(std::string_view path);
    [[nodiscard]] std::shared_ptr<kotavel::session::Context> session_context() const noexcept {
        return session_context_;
    }

private:
    DesktopRouter* routes_;
    WindowManager* windows_;
    std::shared_ptr<kotavel::session::Context> session_context_;
};

// Traduce URIs de esquema registrado a rutas desktop internas.
[[nodiscard]] DispatchResult dispatch_deep_link(const DesktopRouter& router,
                                                std::string_view uri,
                                                std::string_view scheme,
                                                DesktopContext& context);

} // namespace kotavel::desktop

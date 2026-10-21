#include <kotavel/desktop/desktop.hpp>

#include <stdexcept>
#include <utility>

namespace kotavel::desktop {
namespace {
bool valid_segmented_name(std::string_view value, bool allow_slash) {
    if (value.empty()) return false;
    bool previous_separator = true;
    for (const unsigned char ch : value) {
        const bool separator = ch == '.' || (allow_slash && ch == '/');
        const bool valid_character =
            (ch >= 'a' && ch <= 'z') || (ch >= 'A' && ch <= 'Z') ||
            (ch >= '0' && ch <= '9') || ch == '_' || ch == '-';
        if (!separator && !valid_character) return false;
        if (separator && previous_separator) return false;
        previous_separator = separator;
    }
    return !previous_separator;
}

bool valid_route(std::string_view path) {
    if (path == "/") return true;
    if (path.empty() || path.front() != '/' || path.back() == '/') return false;
    return valid_segmented_name(path.substr(1), true);
}

std::string deep_link_path(std::string_view uri, std::string_view scheme) {
    const std::string prefix = std::string(scheme) + "://";
    const std::string bare_scheme = std::string(scheme) + ":";
    if (uri == bare_scheme || uri == bare_scheme + "/" || uri == prefix ||
        uri == prefix + "/")
        return "/";
    if (!uri.starts_with(prefix)) return {};
    const auto destination = uri.substr(prefix.size());
    if (destination.find_first_of("?#") != std::string_view::npos ||
        destination.find('@') != std::string_view::npos ||
        destination.find(':') != std::string_view::npos)
        return {};
    // En kotavel://pos/home, el host pos y el path /home forman /pos/home.
    const auto slash = destination.find('/');
    const auto host = destination.substr(0, slash);
    if (host.empty() || !valid_segmented_name(host, false)) return {};
    const auto suffix = slash == std::string_view::npos ? std::string_view{} : destination.substr(slash);
    std::string path = "/" + std::string(host) + std::string(suffix);
    return valid_route(path) ? path : std::string{};
}
}

View view(std::string_view name) {
    if (!valid_segmented_name(name, false))
        throw std::invalid_argument("Desktop view name is invalid");
    if (!name.starts_with("desktop."))
        throw std::invalid_argument("Desktop views must use the desktop.* namespace");
    return View{std::string(name)};
}

void DesktopRouter::get(std::string_view path, Handler handler) {
    (void)add(path, std::move(handler));
}

std::size_t DesktopRouter::add(std::string_view path, Handler handler) {
    if (!valid_route(path)) throw std::invalid_argument("Desktop route path is invalid");
    if (!handler) throw std::invalid_argument("Desktop route handler cannot be empty");
    if (paths_.contains(std::string(path)))
        throw std::logic_error("Duplicate desktop route: " + std::string(path));
    const auto index = routes_.size();
    routes_.push_back(Definition{std::string(path), {}, std::move(handler)});
    paths_.emplace(routes_.back().path, index);
    return index;
}

void DesktopRouter::set_name(std::size_t index, std::string_view name) {
    if (index >= routes_.size()) throw std::logic_error("No desktop route to name");
    if (!valid_segmented_name(name, true))
        throw std::invalid_argument("Desktop route name is invalid");
    const std::string key(name);
    if (names_.contains(key)) throw std::logic_error("Duplicate desktop route name: " + key);
    auto& route = routes_[index];
    if (!route.name.empty()) names_.erase(route.name);
    route.name = key;
    names_.emplace(route.name, index);
}

DispatchResult DesktopRouter::dispatch(std::string_view path, DesktopContext& context) const {
    if (!valid_route(path)) return DispatchResult::invalid_path;
    const auto found = paths_.find(std::string(path));
    if (found == paths_.end()) return DispatchResult::not_found;
    routes_[found->second].handler(context);
    return DispatchResult::dispatched;
}

bool DesktopRouter::contains(std::string_view path) const {
    return paths_.contains(std::string(path));
}

Route Route::desktop(DesktopRouter& router) { return Route(router); }

Route Route::get(std::string_view path, Handler handler) const {
    auto result = *this;
    result.last_route_index_ = router_->add(path, std::move(handler));
    return result;
}

Route Route::name(std::string_view route_name) const {
    auto result = *this;
    router_->set_name(last_route_index_, route_name);
    return result;
}

DesktopKernel::DesktopKernel(std::unique_ptr<DesktopPlatform> platform,
                             std::filesystem::path views_root)
    : platform_(std::move(platform)), views_root_(std::move(views_root)) {
    if (!platform_) throw std::invalid_argument("Desktop platform cannot be null");
}

int DesktopKernel::run(int argc, char** argv, std::string_view scheme) {
    if (!routes_.contains("/"))
        throw std::logic_error("Desktop startup route '/' is not registered");

    std::string startup_route = "/";
    const std::string prefix = std::string(scheme) + "://";
    std::vector<std::string> platform_arguments;
    platform_arguments.reserve(static_cast<std::size_t>(argc));
    platform_arguments.emplace_back(argc > 0 && argv[0] ? argv[0] : "kotavel");
    for (int index = 1; index < argc; ++index) {
        const std::string_view argument(argv[index] ? argv[index] : "");
        if (argument.starts_with(prefix)) {
            const auto route = deep_link_path(argument, scheme);
            if (route.empty()) throw std::invalid_argument("Desktop deep link is invalid");
            startup_route = route;
        } else if (argument.starts_with("--env=") ||
                   argument.starts_with("--project-root=") ||
                   argument == "--development") {
            // Las opciones del runtime de Kotavel no deben llegar al parser de Qt.
            continue;
        } else {
            platform_arguments.emplace_back(argument);
        }
    }

    if (!routes_.contains(startup_route))
        throw std::logic_error("Desktop startup route is not registered: " + startup_route);

    std::vector<char*> platform_argv;
    platform_argv.reserve(platform_arguments.size());
    for (auto& argument : platform_arguments) platform_argv.push_back(argument.data());
    int platform_argc = static_cast<int>(platform_argv.size());

    platform_->set_deep_link_handler([this, configured_scheme = std::string(scheme)](
                                         std::string_view uri, WindowManager& windows) {
        DesktopContext context(routes_, windows);
        (void)dispatch_deep_link(routes_, uri, configured_scheme, context);
    });

    return platform_->run(platform_argc, platform_argv.data(), views_root_, [this, startup_route](WindowManager& windows) {
        DesktopContext context(routes_, windows);
        const auto result = routes_.dispatch(startup_route, context);
        if (result != DispatchResult::dispatched)
            throw std::logic_error("Desktop startup route could not be dispatched");
    });
}

void DesktopContext::show(std::string_view window_id, const WindowSettings& settings,
                          const View& view_to_show) {
    windows_->show(window_id, settings, view_to_show);
}

void DesktopContext::close(std::string_view window_id) noexcept { windows_->close(window_id); }

bool DesktopContext::is_visible(std::string_view window_id) const noexcept {
    return windows_->is_visible(window_id);
}

DispatchResult DesktopContext::navigate(std::string_view path) {
    return routes_->dispatch(path, *this);
}

DispatchResult dispatch_deep_link(const DesktopRouter& router, std::string_view uri,
                                  std::string_view scheme, DesktopContext& context) {
    const auto path = deep_link_path(uri, scheme);
    if (path.empty()) return DispatchResult::invalid_path;
    return router.dispatch(path, context);
}

} // namespace kotavel::desktop

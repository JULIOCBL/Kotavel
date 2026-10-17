#include <kotavel/desktop/desktop.hpp>

#include <cstdlib>
#include <iostream>
#include <memory>
#include <stdexcept>
#include <string>
#include <unordered_map>
#include <utility>

namespace {
int failures = 0;

void check(bool condition, const char* message) {
    if (!condition) {
        std::cerr << "FAIL: " << message << '\n';
        ++failures;
    }
}

template <class Error, class Function>
void expect_error(Function&& function, const char* message) {
    try {
        function();
        check(false, message);
    } catch (const Error&) {
    } catch (...) {
        check(false, message);
    }
}

class FakeWindows final : public kotavel::desktop::WindowManager {
public:
    void show(std::string_view id, const kotavel::desktop::WindowSettings& settings,
              const kotavel::desktop::View& view) override {
        titles[std::string(id)] = settings.title;
        views[std::string(id)] = view.name;
        visible[std::string(id)] = true;
    }

    void close(std::string_view id) noexcept override { visible[std::string(id)] = false; }

    [[nodiscard]] bool is_visible(std::string_view id) const noexcept override {
        const auto found = visible.find(std::string(id));
        return found != visible.end() && found->second;
    }

    [[nodiscard]] bool has_visible_window() const noexcept override {
        for (const auto& [id, is_open] : visible) {
            (void)id;
            if (is_open) return true;
        }
        return false;
    }

    std::unordered_map<std::string, std::string> titles;
    std::unordered_map<std::string, std::string> views;
    std::unordered_map<std::string, bool> visible;
};

class FakePlatform final : public kotavel::desktop::DesktopPlatform {
public:
    int run(int argc, char** argv, const std::filesystem::path& views_root,
            const StartCallback& start) override {
        received_argc = argc;
        received_root = views_root;
        start(windows);
        return 17;
    }

    int received_argc = 0;
    std::filesystem::path received_root;
    FakeWindows windows;
};

void test_view_validation() {
    check(kotavel::desktop::view("desktop.home").name == "desktop.home",
          "view helper should preserve its logical identifier");
    check(kotavel::desktop::view("desktop.pos.home").name == "desktop.pos.home",
          "view helper should support nested desktop views");
    expect_error<std::invalid_argument>([] { (void)kotavel::desktop::view("web.home"); },
                                        "desktop view helper should reject non-desktop views");
    expect_error<std::invalid_argument>([] { (void)kotavel::desktop::view("desktop../home"); },
                                        "view helper should reject traversal-like names");
}

void test_routes_and_duplicate_validation() {
    kotavel::desktop::DesktopRouter routes;
    auto Route = kotavel::desktop::Route::desktop(routes);
    Route.get("/", [](auto& context) {
        context.show("main", {"Kotavel", 1000, 700}, kotavel::desktop::view("desktop.home"));
    }).name("desktop.home");
    Route.get("/pos/home", [](auto& context) {
        context.show("main", {"Punto de venta", 1200, 800},
                     kotavel::desktop::view("desktop.pos.home"));
    }).name("desktop.pos.home");

    FakeWindows windows;
    kotavel::desktop::DesktopContext context(routes, windows);
    check(routes.dispatch("/", context) == kotavel::desktop::DispatchResult::dispatched,
          "root route should dispatch");
    check(windows.views["main"] == "desktop.home", "root route should show default view");
    check(windows.titles["main"] == "Kotavel", "root route should configure its window title");
    check(routes.dispatch("/pos/home", context) == kotavel::desktop::DispatchResult::dispatched,
          "nested desktop route should dispatch");
    check(windows.views["main"] == "desktop.pos.home", "nested route should replace the view");
    check(routes.dispatch("/missing", context) == kotavel::desktop::DispatchResult::not_found,
          "unknown route should report not_found");
    check(routes.dispatch("pos/home", context) == kotavel::desktop::DispatchResult::invalid_path,
          "desktop paths should require a leading slash");

    expect_error<std::logic_error>([&] {
        routes.get("/", [](auto&) {});
    }, "duplicate desktop paths should fail during registration");
    expect_error<std::invalid_argument>([&] {
        routes.get("/bad?query", [](auto&) {});
    }, "query strings should not be accepted in desktop route definitions");
    expect_error<std::invalid_argument>([&] {
        routes.get("/empty", {});
    }, "empty desktop handlers should be rejected");
    expect_error<std::logic_error>([&] {
        Route.get("/other", [](auto&) {}).name("desktop.home");
    }, "duplicate desktop route names should be rejected");
}

void test_deep_link_translation_and_context_lifetime() {
    kotavel::desktop::DesktopRouter routes;
    routes.get("/pos/home", [](auto& context) {
        context.show("main", {}, kotavel::desktop::view("desktop.pos.home"));
    });
    FakeWindows windows;
    kotavel::desktop::DesktopContext context(routes, windows);
    check(kotavel::desktop::dispatch_deep_link(routes, "kotavel://pos/home", "kotavel", context) ==
              kotavel::desktop::DispatchResult::dispatched,
          "deep links should translate their host and path to desktop routes");
    check(windows.is_visible("main"), "deep-link handler should be able to show a window");
    check(kotavel::desktop::dispatch_deep_link(routes, "https://pos/home", "kotavel", context) ==
              kotavel::desktop::DispatchResult::invalid_path,
          "deep links with another scheme should be rejected");
    check(kotavel::desktop::dispatch_deep_link(routes, "kotavel://pos/unknown", "kotavel", context) ==
              kotavel::desktop::DispatchResult::not_found,
          "well-formed deep links without a route should report not_found");
    check(context.navigate("/pos/home") == kotavel::desktop::DispatchResult::dispatched,
          "desktop context should delegate navigation to the route table");
    context.close("main");
    check(!context.is_visible("main"), "desktop context should close a named window");
}

void test_kernel_default_route_and_startup_failures() {
    auto platform = std::make_unique<FakePlatform>();
    auto* observed_platform = platform.get();
    kotavel::desktop::DesktopKernel kernel(std::move(platform), "resources/views");
    auto Route = kotavel::desktop::Route::desktop(kernel.routes());
    Route.get("/", [](auto& context) {
        context.show("main", {"Inicio", 900, 600}, kotavel::desktop::view("desktop.home"));
    });

    char program[] = "kotavel-lab";
    char* argv[] = {program};
    check(kernel.run(1, argv) == 17, "kernel should return the platform event-loop result");
    check(observed_platform->received_argc == 1, "kernel should pass process arguments to the platform");
    check(observed_platform->received_root == "resources/views", "kernel should pass the configured views root");
    check(observed_platform->windows.views["main"] == "desktop.home",
          "kernel should dispatch / on startup by default");

    kotavel::desktop::DesktopKernel missing_root(std::make_unique<FakePlatform>());
    expect_error<std::logic_error>([&] { (void)missing_root.run(1, argv); },
                                   "desktop startup should fail clearly when / is missing");

    auto deep_link_platform = std::make_unique<FakePlatform>();
    auto* observed_deep_link = deep_link_platform.get();
    kotavel::desktop::DesktopKernel deep_link_kernel(std::move(deep_link_platform));
    auto DeepLinkRoute = kotavel::desktop::Route::desktop(deep_link_kernel.routes());
    DeepLinkRoute.get("/", [](auto& context) {
        context.show("main", {}, kotavel::desktop::view("desktop.home"));
    });
    DeepLinkRoute.get("/pos/home", [](auto& context) {
        context.show("main", {}, kotavel::desktop::view("desktop.pos.home"));
    });
    char app_name[] = "kotavel-lab";
    char deep_link[] = "kotavel://pos/home";
    char* deep_link_argv[] = {app_name, deep_link};
    check(deep_link_kernel.run(2, deep_link_argv) == 17,
          "kernel should start successfully from a registered deep link");
    check(observed_deep_link->windows.views["main"] == "desktop.pos.home",
          "startup deep link should select its matching desktop route");
}
}

int main() {
    try {
        test_view_validation();
        test_routes_and_duplicate_validation();
        test_deep_link_translation_and_context_lifetime();
        test_kernel_default_route_and_startup_failures();
        if (failures != 0) {
            std::cerr << failures << " desktop test(s) failed\n";
            return EXIT_FAILURE;
        }
        std::cout << "Desktop window lifecycle and routing: OK\n";
        return EXIT_SUCCESS;
    } catch (const std::exception& error) {
        std::cerr << "Unexpected desktop test exception: " << error.what() << '\n';
        return EXIT_FAILURE;
    }
}

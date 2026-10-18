#pragma once

#include <functional>
#include <initializer_list>
#include <stdexcept>
#include <string_view>
#include <type_traits>
#include <utility>

#include <kotavel/http/form_request.hpp>
#include <kotavel/http/router.hpp>

namespace kotavel::http {

// Controller::action adapts a member function to a route. FormRequest arguments
// are constructed, authorized, and validated before the controller is called.
class Controller {
public:
    virtual ~Controller() = default;

    template <typename Derived>
    [[nodiscard]] static Handler action(
        Derived& instance, Response (Derived::*method)(const Request&)) {
        return [&instance, method](const Request& current) {
            return (instance.*method)(current);
        };
    }

    template <typename Derived>
    [[nodiscard]] static Handler action(
        Derived& instance, Response (Derived::*method)(const Request&) const) {
        return [&instance, method](const Request& current) {
            return (instance.*method)(current);
        };
    }

    template <typename Derived, typename Form>
        requires std::is_base_of_v<FormRequest, Form>
    [[nodiscard]] static Handler action(
        Derived& instance, Response (Derived::*method)(const Form&)) {
        return [&instance, method](const Request& current) {
            Form form(current);
            if (!form.validate()) return form.error_response();
            return (instance.*method)(form);
        };
    }

    template <typename Derived, typename Form>
        requires std::is_base_of_v<FormRequest, Form>
    [[nodiscard]] static Handler action(
        Derived& instance, Response (Derived::*method)(const Form&) const) {
        return [&instance, method](const Request& current) {
            Form form(current);
            if (!form.validate()) return form.error_response();
            return (instance.*method)(form);
        };
    }
};

// Route context that turns typed controller member functions into handlers.
// The controller instance must outlive the registered routes.
template <typename ControllerType>
class ControllerRoutes final {
public:
    using Group = std::function<void(ControllerRoutes&)>;

    [[nodiscard]] ControllerRoutes prefix(std::string_view value) const {
        return ControllerRoutes(routes_.prefix(value), *instance_);
    }

    [[nodiscard]] ControllerRoutes middleware(Middleware middleware) const {
        return ControllerRoutes(routes_.middleware(std::move(middleware)), *instance_);
    }

    [[nodiscard]] ControllerRoutes middleware(std::string_view alias) const {
        return ControllerRoutes(routes_.middleware(alias), *instance_);
    }

    [[nodiscard]] ControllerRoutes middleware(
        std::initializer_list<std::string_view> aliases) const {
        return ControllerRoutes(routes_.middleware(aliases), *instance_);
    }

    void group(const Group& callback) const {
        if (!callback) throw std::invalid_argument("Controller route group callback cannot be empty");
        auto scoped_routes = *this;
        callback(scoped_routes);
    }

    template <typename Method>
    Route get(std::string_view path, Method method) const {
        return routes_.get(path, Controller::action(*instance_, method));
    }

    template <typename Method>
    Route post(std::string_view path, Method method) const {
        return routes_.post(path, Controller::action(*instance_, method));
    }

    template <typename Method>
    Route put(std::string_view path, Method method) const {
        return routes_.put(path, Controller::action(*instance_, method));
    }

    template <typename Method>
    Route patch(std::string_view path, Method method) const {
        return routes_.patch(path, Controller::action(*instance_, method));
    }

    template <typename Method>
    Route remove(std::string_view path, Method method) const {
        return routes_.remove(path, Controller::action(*instance_, method));
    }

private:
    friend class Route;
    ControllerRoutes(Route routes, ControllerType& instance)
        : routes_(std::move(routes)), instance_(&instance) {}

    Route routes_;
    ControllerType* instance_;
};

template <typename ControllerType>
ControllerRoutes<ControllerType> Route::controller(ControllerType& instance) const {
    return ControllerRoutes<ControllerType>(*this, instance);
}

} // namespace kotavel::http

#include <kotavel/http/server.hpp>

#include <drogon/drogon.h>
#include <atomic>
#include <chrono>
#include <optional>
#include <stdexcept>
#include <utility>

namespace kotavel::http {
namespace {
std::atomic_bool server_claimed = false;
constexpr const char* request_timing_attribute = "kotavel.http.request_timing";

struct RequestTiming {
    std::chrono::system_clock::time_point received_at;
    std::chrono::steady_clock::time_point started_at;
};

std::string_view request_method(const drogon::HttpRequestPtr& request) {
    return request->isHead() ? "HEAD" : request->methodString();
}

bool route_path_matches(std::string_view pattern, std::string_view path) {
    std::size_t pattern_start = 0;
    std::size_t path_start = 0;
    while (pattern_start < pattern.size() || path_start < path.size()) {
        if (pattern_start == pattern.size() || path_start == path.size())
            return pattern_start == pattern.size() && path_start == path.size();

        const auto pattern_end = pattern.find('/', pattern_start);
        const auto path_end = path.find('/', path_start);
        const auto pattern_segment = pattern.substr(
            pattern_start, pattern_end == std::string_view::npos
                ? pattern.size() - pattern_start : pattern_end - pattern_start);
        const auto path_segment = path.substr(
            path_start, path_end == std::string_view::npos
                ? path.size() - path_start : path_end - path_start);

        const bool parameter = pattern_segment.size() >= 2 &&
            pattern_segment.front() == '{' && pattern_segment.back() == '}';
        if (!parameter && pattern_segment != path_segment) return false;
        if (parameter && path_segment.empty()) return false;

        if (pattern_end == std::string_view::npos || path_end == std::string_view::npos)
            return pattern_end == path_end;
        pattern_start = pattern_end + 1;
        path_start = path_end + 1;
    }
    return true;
}

std::optional<Method> to_kotavel_method(drogon::HttpMethod method) {
    switch (method) {
        case drogon::Get:
        case drogon::Head: return Method::get;
        case drogon::Post: return Method::post;
        case drogon::Put: return Method::put;
        case drogon::Patch: return Method::patch;
        case drogon::Delete: return Method::delete_;
        default: return std::nullopt;
    }
}

drogon::HttpMethod to_drogon_method(Method method) {
    switch (method) {
        case Method::get: return drogon::Get;
        case Method::post: return drogon::Post;
        case Method::put: return drogon::Put;
        case Method::patch: return drogon::Patch;
        case Method::delete_: return drogon::Delete;
    }
    return drogon::Get;
}

drogon::HttpResponsePtr to_drogon_response(Response response) {
    auto result = drogon::HttpResponse::newHttpResponse();
    const auto status = response.status >= 100 && response.status <= 599
        ? response.status : 500;
    result->setStatusCode(static_cast<drogon::HttpStatusCode>(status));
    result->setContentTypeString(response.content_type);
    result->setBody(std::move(response.body));
    return result;
}

Request request_from_transport(const drogon::HttpRequestPtr& incoming,
                               const RouteDefinition& route) {
    RequestData data;
    data.method = route.method;
    data.kind = route.kind;
    data.path = incoming->path();
    data.body = std::string(incoming->body());
    data.raw_method = std::string(request_method(incoming));
    data.query_string = incoming->query();
    for (const auto& [name, value] : incoming->headers())
        data.headers.insert_or_assign(name, value);

    const auto& captures = incoming->getRoutingParameters();
    std::size_t capture_index = 0;
    std::size_t segment_start = 0;
    const std::string_view pattern(route.path);
    while (segment_start < pattern.size()) {
        const auto segment_end = pattern.find('/', segment_start);
        const auto segment = pattern.substr(segment_start,
            segment_end == std::string_view::npos
                ? pattern.size() - segment_start : segment_end - segment_start);
        if (segment.size() >= 2 && segment.front() == '{' && segment.back() == '}' &&
            capture_index < captures.size()) {
            const std::string key(segment.substr(1, segment.size() - 2));
            data.route_parameters.insert_or_assign(key, captures[capture_index++]);
        }
        if (segment_end == std::string_view::npos) break;
        segment_start = segment_end + 1;
    }
    return Request(std::move(data));
}

Request request_from_transport(const drogon::HttpRequestPtr& incoming) {
    const auto path = incoming->path();
    RequestData data;
    data.kind = path == "/api" || path.starts_with("/api/") ? RouteKind::api : RouteKind::web;
    data.path = path;
    data.body = std::string(incoming->body());
    data.raw_method = std::string(request_method(incoming));
    data.query_string = incoming->query();
    for (const auto& [name, value] : incoming->headers())
        data.headers.insert_or_assign(name, value);
    return Request(std::move(data));
}

class DrogonServer final : public Server {
public:
    void run(const HttpKernel& kernel, const ServerSettings& settings,
             const ServerStartedCallback& started) override {
        bool expected = false;
        if (!server_claimed.compare_exchange_strong(expected, true))
            throw std::logic_error("Only one HTTP server can run in a process");

        try {
            auto& application = drogon::app();
            if (settings.threads > 0) application.setThreadNum(settings.threads);
            application.registerPreRoutingAdvice([&kernel](
                    const drogon::HttpRequestPtr& request,
                    drogon::AdviceCallback&& callback,
                    drogon::AdviceChainCallback&& chain) {
                const auto& routes = kernel.routes().routes();
                bool path_is_registered = false;
                bool method_is_registered = false;
                const auto method = to_kotavel_method(request->method());
                for (const auto& route : routes) {
                    if (!route_path_matches(route.path, request->path())) continue;
                    path_is_registered = true;
                    if (method && route.method == *method) {
                        method_is_registered = true;
                        break;
                    }
                }
                if (!path_is_registered || method_is_registered) {
                    chain();
                    return;
                }

                const auto view = request_from_transport(request);
                callback(to_drogon_response(kernel.method_not_allowed(view)));
            });
            application.registerPreRoutingAdvice([](const drogon::HttpRequestPtr& request) {
                request->attributes()->insert(request_timing_attribute,
                    RequestTiming{std::chrono::system_clock::now(),
                                  std::chrono::steady_clock::now()});
            });
            application.registerPreSendingAdvice([&kernel](
                    const drogon::HttpRequestPtr& request,
                    const drogon::HttpResponsePtr& response) {
                const auto now_steady = std::chrono::steady_clock::now();
                const auto now_system = std::chrono::system_clock::now();
                const auto& attributes = request->attributes();
                RequestTiming timing{now_system, now_steady};
                if (attributes && attributes->find(request_timing_attribute))
                    timing = attributes->get<RequestTiming>(request_timing_attribute);
                const auto view = request_from_transport(request);
                kernel.log_access(view, static_cast<int>(response->statusCode()),
                                  timing.received_at, now_steady - timing.started_at);
            });
            for (const auto& route : kernel.routes().routes()) {
                const auto route_definition = route;
                application.registerHandler(route.path,
                    [&kernel, route_definition](const drogon::HttpRequestPtr& request,
                              std::function<void(const drogon::HttpResponsePtr&)>&& callback) {
                        const auto view = request_from_transport(request, route_definition);
                        callback(to_drogon_response(kernel.handle(route_definition, view)));
                    }, {to_drogon_method(route_definition.method)});
            }
            application.setDefaultHandler(
                [&kernel](const drogon::HttpRequestPtr& request,
                          std::function<void(const drogon::HttpResponsePtr&)>&& callback) {
                    const auto view = request_from_transport(request);
                    callback(to_drogon_response(kernel.not_found(view)));
                });
            application.addListener(settings.host, settings.port);
            if (started) application.registerBeginningAdvice(started);
            application.run();
        } catch (...) {
            server_claimed.store(false);
            throw;
        }
        server_claimed.store(false);
    }

    void stop() noexcept override {
        if (server_claimed.load()) drogon::app().quit();
    }
};
}

std::unique_ptr<Server> make_server() {
    return std::make_unique<DrogonServer>();
}

} // namespace kotavel::http

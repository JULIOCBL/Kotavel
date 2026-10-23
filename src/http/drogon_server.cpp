#include <kotavel/http/server.hpp>

#include "route_method_index.hpp"

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

void copy_request_headers(const drogon::HttpRequestPtr& incoming, RequestData& data) {
    for (const auto& [name, value] : incoming->headers())
        data.headers.insert_or_assign(name, value);

    // Preserve the raw Cookie header when the adapter exposes it. Some Drogon
    // request paths expose parsed cookies separately, so rebuild the standard
    // header from that parsed map as a fallback for framework middleware.
    const auto& cookie = incoming->getHeader("cookie");
    if (!cookie.empty()) {
        data.headers.insert_or_assign("cookie", cookie);
        return;
    }

    std::string parsed_cookie_header;
    for (const auto& [name, value] : incoming->cookies()) {
        if (!parsed_cookie_header.empty()) parsed_cookie_header += "; ";
        parsed_cookie_header += name;
        parsed_cookie_header += '=';
        parsed_cookie_header += value;
    }
    if (!parsed_cookie_header.empty())
        data.headers.insert_or_assign("cookie", std::move(parsed_cookie_header));
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
    for (const auto& [name, value] : response.headers) result->addHeader(name, value);
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
    copy_request_headers(incoming, data);

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

// Error negotiation only needs the method, path and Accept header. Do not copy
// or parse an upload merely to return 404/405.
Request request_metadata_from_transport(const drogon::HttpRequestPtr& incoming) {
    RequestData data;
    data.method = to_kotavel_method(incoming->method()).value_or(Method::get);
    data.path = incoming->path();
    data.kind = data.path == "/api" || data.path.starts_with("/api/") ? RouteKind::api : RouteKind::web;
    data.raw_method = std::string(request_method(incoming));
    const auto& accept = incoming->getHeader("accept");
    if (!accept.empty()) data.headers.emplace("accept", accept);
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
            const auto route_index = std::make_shared<const detail::RouteMethodIndex>(kernel.routes().routes());
            application.registerPreRoutingAdvice([&kernel, route_index](
                    const drogon::HttpRequestPtr& request,
                    drogon::AdviceCallback&& callback,
                    drogon::AdviceChainCallback&& chain) {
                request->attributes()->insert(request_timing_attribute,
                    RequestTiming{std::chrono::system_clock::now(), std::chrono::steady_clock::now()});
                if (!route_index->rejects(request->path(), to_kotavel_method(request->method()))) {
                    chain();
                    return;
                }
                const auto view = request_metadata_from_transport(request);
                callback(to_drogon_response(kernel.method_not_allowed(view)));
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
                kernel.log_access(request_method(request), request->path(),
                                  static_cast<int>(response->statusCode()),
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
                    const auto view = request_metadata_from_transport(request);
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

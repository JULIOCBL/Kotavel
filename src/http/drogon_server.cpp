#include <kotavel/http/server.hpp>

#include <drogon/drogon.h>
#include <atomic>
#include <stdexcept>
#include <utility>

namespace kotavel::http {
namespace {
std::atomic_bool server_claimed = false;

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
            for (const auto& route : kernel.routes().routes()) {
                const auto route_definition = route;
                application.registerHandler(route.path,
                    [&kernel, route_definition](const drogon::HttpRequestPtr& request,
                              std::function<void(const drogon::HttpResponsePtr&)>&& callback) {
                        const RequestView view{route_definition.method,
                                               request->path(), request->body()};
                        callback(to_drogon_response(kernel.handle(route_definition, view)));
                    }, {to_drogon_method(route_definition.method)});
            }
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

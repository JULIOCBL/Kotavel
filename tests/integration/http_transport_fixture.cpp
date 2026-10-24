#include <kotavel/http/api_response.hpp>
#include <kotavel/http/server.hpp>
#include <kotavel/http/session_middleware.hpp>
#include <kotavel/session/session.hpp>

#include <drogon/drogon.h>

#include <cstdint>
#include <functional>
#include <iostream>
#include <string>

using namespace kotavel::http;

int main(int argc, char** argv) {
    if (argc != 3) return 2;
    try {
        auto& application = drogon::app();
        application.enableSession(120, drogon::Cookie::SameSite::kLax, "drogon_session");
        application.registerHandler("/drogon-session",
            [](const drogon::HttpRequestPtr& request,
               std::function<void(const drogon::HttpResponsePtr&)>&& callback) {
                std::uint64_t count = 0;
                request->session()->modify<std::uint64_t>("visits", [&count](std::uint64_t& value) {
                    count = ++value;
                });
                auto response = drogon::HttpResponse::newHttpResponse();
                response->setBody(std::to_string(count));
                callback(response);
            }, {drogon::Get});
        HttpKernel kernel(make_server());
        SessionConfig sessions;
        sessions.database = std::filesystem::path(argv[2]) / "sessions.sqlite3";
        kernel.use_web(start_configured_session(sessions, "."));
        for (int i = 0; i < 1000; ++i)
            kernel.routes().api_get("/static/" + std::to_string(i), [](const Request&) {
                return ApiResponse::success("ok");
            });
        kernel.routes().api_post("/echo", [](const Request& request) {
            return ApiResponse::success(request.all());
        });
        kernel.routes().api_get("/items/{id}", [](const Request& request) {
            return ApiResponse::success(std::string(request.route("id").value_or("")));
        });
        kernel.routes().api_post("/items/special", [](const Request&) {
            return ApiResponse::success("created", 201);
        });
        kernel.routes().web_get("/session", [](const Request&) {
            Session::increment("visits");
            const int count = Session::get<int>("visits", 0);
            return Response{200, "text/plain", std::to_string(count)};
        });
        kernel.routes().web_get("/session/read", [](const Request&) {
            const int count = Session::get<int>("visits", 0);
            return Response{200, "text/plain", std::to_string(count)};
        });
        kernel.routes().web_get("/session/write", [](const Request&) {
            Session::increment("visits");
            const int count = Session::get<int>("visits", 0);
            return Response{200, "text/plain", std::to_string(count)};
        });
        kernel.run({"127.0.0.1", static_cast<std::uint16_t>(std::stoi(argv[1])), 16});
    } catch (const std::exception& error) {
        std::cerr << error.what() << '\n';
        return 1;
    }
}

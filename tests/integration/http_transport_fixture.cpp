#include <kotavel/http/api_response.hpp>
#include <kotavel/http/server.hpp>
#include <kotavel/http/session_middleware.hpp>
#include <kotavel/session/session.hpp>

#include <iostream>

using namespace kotavel::http;

int main(int argc, char** argv) {
    if (argc != 3) return 2;
    try {
        HttpKernel kernel(make_server());
        SessionConfig sessions;
        sessions.files = argv[2];
        kernel.use_web(start_file_session(sessions, "."));
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
            const int count = Session::get<int>("visits", 0) + 1;
            Session::put("visits", count);
            return Response{200, "text/plain", std::to_string(count)};
        });
        kernel.run({"127.0.0.1", static_cast<std::uint16_t>(std::stoi(argv[1])), 2});
    } catch (const std::exception& error) {
        std::cerr << error.what() << '\n';
        return 1;
    }
}

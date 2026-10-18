#include <kotavel/http/api_response.hpp>
#include <kotavel/http/server.hpp>

#include <iostream>
#include <memory>
#include <stdexcept>
#include <string>
#include <string_view>
#include <vector>

namespace {
void check(bool condition, const std::string& message) {
    if (!condition) throw std::runtime_error(message);
}

class FakeServer final : public kotavel::http::Server {
public:
    void run(const kotavel::http::HttpKernel&, const kotavel::http::ServerSettings&,
             const kotavel::http::ServerStartedCallback&) override {}
    void stop() noexcept override {}
};

kotavel::http::Request make_request(kotavel::http::RouteKind kind,
                                    std::string path,
                                    std::string accept = {},
                                    std::string method = "GET",
                                    std::string body = {}) {
    kotavel::http::RequestData data;
    data.kind = kind;
    data.path = std::move(path);
    data.raw_method = std::move(method);
    data.body = std::move(body);
    if (!accept.empty()) data.headers.emplace("Accept", std::move(accept));
    if (!data.body.empty()) data.headers.emplace("Content-Type", "application/json");
    return kotavel::http::Request(std::move(data));
}

void test_default_representations() {
    kotavel::http::HttpKernel kernel(std::make_unique<FakeServer>());
    int api_calls = 0;
    int web_calls = 0;
    kernel.routes().api_get("/health", [&](const auto&) {
        ++api_calls;
        return kotavel::http::ApiResponse::success(std::string("ok"));
    });
    kernel.routes().web_get("/", [&](const auto&) {
        ++web_calls;
        return kotavel::http::Response{200, "text/html; charset=utf-8", "<h1>home</h1>"};
    });

    const auto& api = kernel.routes().routes()[0];
    const auto& web = kernel.routes().routes()[1];
    for (const auto accept : {std::string{}, std::string(" ")}) {
        const auto response = kernel.handle(api, make_request(
            kotavel::http::RouteKind::api, "/api/health", accept));
        check(response.status == 200 &&
              response.content_type == "application/json; charset=utf-8" &&
              response.body == R"({"data":"ok"})",
              "an absent or blank Accept header should use the API JSON default");
    }

    const auto page = kernel.handle(web, make_request(kotavel::http::RouteKind::web, "/"));
    check(page.status == 200 && page.content_type == "text/html; charset=utf-8" &&
          page.body == "<h1>home</h1>" && api_calls == 2 && web_calls == 1,
          "web routes should use the HTML default when Accept is absent");
}

void test_api_accept_matrix() {
    kotavel::http::HttpKernel kernel(std::make_unique<FakeServer>());
    int middleware_calls = 0;
    int handler_calls = 0;
    kernel.use_api([&](const auto&, const auto& next) {
        ++middleware_calls;
        return next();
    });
    kernel.routes().api_get("/items", [&](const auto&) {
        ++handler_calls;
        return kotavel::http::ApiResponse::success(std::string("items"));
    });
    const auto& route = kernel.routes().routes().front();

    struct Case {
        std::string accept;
        bool allowed;
        const char* description;
    };
    const std::vector<Case> cases{
        {"application/json", true, "exact media type"},
        {"Application/JSON", true, "case-insensitive media type"},
        {"application/json; charset=utf-8", true, "media type parameters"},
        {"application/json;q=0.001", true, "small positive quality"},
        {"text/html, application/json;q=0.4", true, "list with a supported member"},
        {"application/*", true, "type wildcard"},
        {"*/*", true, "global wildcard"},
        {"application/json;q=0, */*;q=1", false, "specific q=0 overrides wildcard"},
        {"application/*;q=0, */*;q=1", false, "type wildcard q=0 overrides global wildcard"},
        {"application/json;q=0", false, "explicit q=0"},
        {"*/*;q=0", false, "global q=0"},
        {"text/html", false, "unsupported media type"},
        {"application/json;q=bogus", false, "malformed quality value"},
        {"application/json;q=1.1", false, "quality above one"},
        {"not-a-media-type", false, "malformed media range"},
    };

    for (const auto& test : cases) {
        const int before_handler = handler_calls;
        const int before_middleware = middleware_calls;
        const auto response = kernel.handle(route, make_request(
            kotavel::http::RouteKind::api, "/api/items", test.accept));
        const auto expected_status = test.allowed ? 200 : 406;
        check(response.status == expected_status,
              std::string("wrong status for API Accept case: ") + test.description);
        if (test.allowed) {
            check(response.content_type == "application/json; charset=utf-8" &&
                  handler_calls == before_handler + 1 &&
                  middleware_calls == before_middleware + 1,
                  std::string("accepted API format did not reach middleware and handler: ") +
                      test.description);
        } else {
            check(response.content_type == "application/json; charset=utf-8" &&
                  response.body.find(R"("status":"406")") != std::string::npos &&
                  response.body.find(R"("title":"Not Acceptable")") != std::string::npos &&
                  handler_calls == before_handler && middleware_calls == before_middleware,
                  std::string("API 406 must be standard JSON and stop execution: ") +
                      test.description);
        }
    }
}

void test_web_accept_matrix() {
    kotavel::http::HttpKernel kernel(std::make_unique<FakeServer>());
    int handler_calls = 0;
    kernel.routes().web_get("/", [&](const auto&) {
        ++handler_calls;
        return kotavel::http::Response{200, "text/html; charset=utf-8", "<p>ok</p>"};
    });
    const auto& route = kernel.routes().routes().front();

    for (const auto accept : {"text/html", "TEXT/HTML", "text/*", "*/*",
                              "text/html,application/xhtml+xml;q=0.9,*/*;q=0.8"}) {
        const auto response = kernel.handle(route, make_request(
            kotavel::http::RouteKind::web, "/", accept));
        check(response.status == 200 && response.content_type == "text/html; charset=utf-8",
              std::string("browser-compatible Accept should allow HTML: ") + accept);
    }
    check(handler_calls == 5, "all accepted web representations should invoke the handler");

    for (const auto accept : {"application/json", "text/html;q=0, */*;q=1", "*/*;q=0"}) {
        const auto before = handler_calls;
        const auto response = kernel.handle(route, make_request(
            kotavel::http::RouteKind::web, "/", accept));
        check(response.status == 406 && response.content_type == "text/html; charset=utf-8" &&
              response.body.find("406 Not Acceptable") != std::string::npos &&
              handler_calls == before,
              std::string("unacceptable web preference should return HTML 406: ") + accept);
    }
}

void test_automatic_error_negotiation() {
    kotavel::http::HttpKernel kernel(std::make_unique<FakeServer>());

    const auto api_404 = kernel.not_found(make_request(
        kotavel::http::RouteKind::api, "/api/missing", "application/json"));
    check(api_404.status == 404 && api_404.content_type == "application/json; charset=utf-8" &&
          api_404.body.find(R"("status":"404")") != std::string::npos,
          "unknown API routes should retain the standard JSON 404 when accepted");
    const auto api_404_rejected = kernel.not_found(make_request(
        kotavel::http::RouteKind::api, "/api/missing", "text/html"));
    check(api_404_rejected.status == 406 &&
          api_404_rejected.content_type == "application/json; charset=utf-8",
          "unknown API routes should return 406 when JSON is excluded");

    const auto web_404 = kernel.not_found(make_request(
        kotavel::http::RouteKind::web, "/missing", "text/html"));
    check(web_404.status == 404 && web_404.content_type == "text/html; charset=utf-8",
          "unknown web routes should retain the HTML 404 when accepted");
    const auto web_404_rejected = kernel.not_found(make_request(
        kotavel::http::RouteKind::web, "/missing", "application/json"));
    check(web_404_rejected.status == 406 &&
          web_404_rejected.content_type == "text/html; charset=utf-8",
          "unknown web routes should return 406 when HTML is excluded");

    const auto api_405 = kernel.method_not_allowed(make_request(
        kotavel::http::RouteKind::api, "/api/items", "application/json", "POST"));
    check(api_405.status == 405 && api_405.content_type == "application/json; charset=utf-8",
          "automatic API 405 responses should use JSON");
    const auto api_405_rejected = kernel.method_not_allowed(make_request(
        kotavel::http::RouteKind::api, "/api/items", "text/html", "POST"));
    check(api_405_rejected.status == 406 &&
          api_405_rejected.content_type == "application/json; charset=utf-8",
          "automatic API 405 responses should negotiate Accept");
    const auto options = kernel.method_not_allowed(make_request(
        kotavel::http::RouteKind::web, "/", "text/html", "OPTIONS"));
    check(options.status == 403 && options.content_type == "text/html; charset=utf-8",
          "the existing OPTIONS policy should preserve its web representation");

    int bad_json_handler_calls = 0;
    kernel.routes().api_post("/body", [&](const auto&) {
        ++bad_json_handler_calls;
        return kotavel::http::ApiResponse::success(std::string("unexpected"));
    });
    const auto& body_route = kernel.routes().routes().back();
    const auto rejected_before_body_validation = kernel.handle(body_route, make_request(
        kotavel::http::RouteKind::api, "/api/body", "text/html", "POST", "{"));
    check(rejected_before_body_validation.status == 406 && bad_json_handler_calls == 0,
          "unacceptable formats should be rejected before request-body validation");
    const auto bad_json = kernel.handle(body_route, make_request(
        kotavel::http::RouteKind::api, "/api/body", "application/json", "POST", "{"));
    check(bad_json.status == 400 &&
          bad_json.content_type == "application/json; charset=utf-8" &&
          bad_json_handler_calls == 0,
          "invalid API JSON should preserve its JSON error response when accepted");
}
} // namespace

int main() {
    try {
        test_default_representations();
        test_api_accept_matrix();
        test_web_accept_matrix();
        test_automatic_error_negotiation();
        std::cout << "HTTP content negotiation: OK\n";
        return 0;
    } catch (const std::exception& error) {
        std::cerr << error.what() << '\n';
        return 1;
    }
}

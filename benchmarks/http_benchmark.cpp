#include <kotavel/http/form_request.hpp>
#include <kotavel/http/server.hpp>

#include <algorithm>
#include <chrono>
#include <iostream>
#include <stdexcept>
#include <vector>

using namespace kotavel::http;
using Clock = std::chrono::steady_clock;

namespace {
template<class Action> void measure(const char* name, Action action, std::size_t batch = 1) {
    constexpr std::size_t count = 1000;
    for (int i = 0; i < 100; ++i) action();
    std::vector<double> samples;
    samples.reserve(count);
    const auto start = Clock::now();
    for (std::size_t i = 0; i < count; ++i) {
        const auto begin = Clock::now();
        for (std::size_t j = 0; j < batch; ++j) action();
        samples.push_back(std::chrono::duration<double, std::nano>(Clock::now() - begin).count() / batch);
    }
    const auto seconds = std::chrono::duration<double>(Clock::now() - start).count();
    std::sort(samples.begin(), samples.end());
    std::cout << name << ',' << count << ',' << batch << ',' << samples[count / 2] << ','
              << samples[count * 95 / 100] << ',' << samples[count * 99 / 100] << ','
              << count * batch / seconds << '\n';
}

class NestedForm final : public FormRequest {
public:
    using FormRequest::FormRequest;
    ValidationRules rules() const override {
        ValidationRules result;
        for (int i = 0; i < 40; ++i)
            result.emplace("profile_attribute_" + std::to_string(i), std::vector<std::string>{"required", "array"});
        return result;
    }
};

class NoopServer final : public Server {
    void run(const HttpKernel&, const ServerSettings&, const ServerStartedCallback&) override {}
    void stop() noexcept override {}
};
}

int main() {
    try {
        std::cout << "scenario,samples,operations_per_sample,p50_ns,p95_ns,p99_ns,operations_per_second\n";
        RequestData data;
        data.kind = RouteKind::api;
        data.method = Method::post;
        data.path = "/api/benchmark";
        data.headers = {{"Content-Type", "application/json"}, {"Accept", "application/json"}};
        data.body = "{";
        for (int i = 0; i < 40; ++i) {
            if (i) data.body += ',';
            data.body += "\"profile_attribute_" + std::to_string(i) + "\":{\"name\":\"Kotavel\",\"value\":42}";
        }
        data.body += '}';
        measure("request_parse_nested_json", [&] {
            Request request(data);
            if (!request.has_valid_input() || request.input("profile_attribute_39.name") != "Kotavel")
                throw std::runtime_error("invalid parsed input");
        });
        const Request request(data);
        measure("request_lookup_long_key", [&] {
            if (!request.exists("profile_attribute_39.name") ||
                request.input("profile_attribute_39.name") != "Kotavel")
                throw std::runtime_error("lookup failed");
        }, 100);
        measure("form_validate_nested", [&] {
            NestedForm form(request);
            if (!form.validate() || form.validated().size() != 80)
                throw std::runtime_error("validation failed");
        });
        std::vector<ApiError> errors;
        for (int i = 0; i < 40; ++i)
            errors.push_back({"Validation Failed", "The field is required.", "field_" + std::to_string(i)});
        measure("response_validation_errors", [&] {
            const auto response = ApiResponse::error(422, errors);
            if (response.status != 422 || response.body.empty()) throw std::runtime_error("invalid response");
        });
        HttpKernel kernel(std::make_unique<NoopServer>());
        kernel.routes().api_post("/benchmark", [](const Request&) { return ApiResponse::success("ok"); });
        for (int i = 0; i < 5; ++i)
            kernel.use_api([](const Request&, const Next& next) { return next(); });
        measure("kernel_five_middleware", [&] {
            if (kernel.handle(kernel.routes().routes().front(), request).status != 200)
                throw std::runtime_error("dispatch failed");
        }, 10);
    } catch (const std::exception& error) {
        std::cerr << error.what() << '\n';
        return 1;
    }
}

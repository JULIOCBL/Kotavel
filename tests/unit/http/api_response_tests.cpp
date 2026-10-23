#include <kotavel/http/api_response.hpp>

#include <iostream>
#include <map>
#include <stdexcept>
#include <string>
#include <unordered_map>
#include <vector>

namespace {
void check(bool condition, const char* message) {
    if (!condition) throw std::runtime_error(message);
}

struct User {
    int id;
    std::string name;
};
}

template <>
struct kotavel::http::ApiJsonSerializer<User> {
    static kotavel::http::JsonValue serialize(const User& user) {
        return kotavel::http::JsonValue::Object{
            {"id", user.id},
            {"name", user.name},
        };
    }
};

int main() {
    using kotavel::http::ApiError;
    using kotavel::http::ApiResponse;

    const std::vector<User> users{{7, "Ada \"Lovelace\""}, {8, "Grace"}};
    const auto vector_response = ApiResponse::success(users, 200, "Users", "Loaded");
    check(vector_response.status == 200 &&
          vector_response.content_type == "application/json; charset=utf-8" &&
          vector_response.body ==
          R"({"data":[{"id":7,"name":"Ada \"Lovelace\""},{"id":8,"name":"Grace"}],"meta":{"message":"Loaded","title":"Users"}})",
          "success should serialize vectors, custom structs and metadata into a JSON envelope");

    const std::map<std::string, int> counts{{"active", 3}, {"pending", 2}};
    const auto map_response = ApiResponse::success(counts);
    check(map_response.body == R"({"data":{"active":3,"pending":2}})",
          "success should serialize std::map as a JSON object");

    const std::unordered_map<int, std::string> labels{{1, "one"}, {2, "two"}};
    const auto unordered_response = ApiResponse::success(labels);
    check(unordered_response.body == R"({"data":{"1":"one","2":"two"}})",
          "success should convert integral unordered_map keys to JSON object keys");

    const auto error_response = ApiResponse::error(422, std::vector<ApiError>{
        {"Validation Failed", "The email is required", std::string("email")},
        {"Validation Failed", "The name is required", std::string("name")}});
    check(error_response.status == 422 &&
          error_response.body ==
          R"({"errors":[{"detail":"The email is required","source":{"field":"email"},"status":"422","title":"Validation Failed"},{"detail":"The name is required","source":{"field":"name"},"status":"422","title":"Validation Failed"}]})",
          "error should support standard validation errors with field sources");

    const auto no_content = ApiResponse::success(users, 204);
    check(no_content.status == 204 && no_content.body.empty(),
          "204 success responses should not contain a body");

    const auto escaped = ApiResponse::error(400, std::vector<ApiError>{
        {"Bad \"input\"", std::string("a\0b\n", 4), std::string{}}});
    check(escaped.body ==
          R"({"errors":[{"detail":"a\u0000b\n","source":{"field":""},"status":"400","title":"Bad \"input\""}]})",
          "direct error serialization must preserve escaping, embedded nulls and an empty field source");

    bool rejected_status = false;
    try { (void)ApiResponse::error(200, "Bad", "Invalid error status"); }
    catch (const std::invalid_argument&) { rejected_status = true; }
    check(rejected_status, "error should reject non-error HTTP statuses");

    std::cout << "API response tests passed\n";
}

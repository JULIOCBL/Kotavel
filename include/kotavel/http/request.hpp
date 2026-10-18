#pragma once

#include <map>
#include <optional>
#include <set>
#include <string>
#include <string_view>
#include <vector>

namespace kotavel::http {

enum class Method { get, post, put, patch, delete_ };
enum class RouteKind { api, web };

struct RequestData final {
    Method method = Method::get;
    RouteKind kind = RouteKind::web;
    std::string path = "/";
    std::string body;
    std::string raw_method;
    std::string query_string;
    std::map<std::string, std::string> headers;
    std::map<std::string, std::string> route_parameters;
};

struct UploadedFile final {
    std::string name;
    std::string content_type;
    std::string contents;
};

// Request owns transport data so controller and form request objects can safely
// inspect it for the full synchronous lifetime of a handler.
class Request {
public:
    explicit Request(RequestData data);
    Request(Method method, std::string_view path, std::string_view body,
            std::string_view raw_method = {}, RouteKind kind = RouteKind::web);

    [[nodiscard]] Method method() const noexcept { return method_; }
    [[nodiscard]] RouteKind kind() const noexcept { return kind_; }
    [[nodiscard]] std::string_view path() const noexcept { return path_; }
    [[nodiscard]] std::string_view raw_method() const noexcept { return raw_method_; }
    [[nodiscard]] std::string_view body() const noexcept { return body_; }
    [[nodiscard]] std::string_view query_string() const noexcept { return query_string_; }

    // input() searches parsed body values first, then query parameters.
    [[nodiscard]] std::optional<std::string_view> input(std::string_view key) const;
    [[nodiscard]] std::optional<std::string_view> query(std::string_view key) const;
    [[nodiscard]] std::optional<std::string_view> route(std::string_view key) const;
    [[nodiscard]] std::optional<std::string_view> header(std::string_view key) const;
    [[nodiscard]] const UploadedFile* file(std::string_view key) const;
    [[nodiscard]] bool exists(std::string_view key) const;
    [[nodiscard]] bool is_null(std::string_view key) const;
    [[nodiscard]] bool has(std::string_view key) const;
    [[nodiscard]] bool filled(std::string_view key) const;
    [[nodiscard]] bool is_array(std::string_view key) const;
    [[nodiscard]] bool is_object(std::string_view key) const;
    [[nodiscard]] std::vector<std::string> array_values(std::string_view key) const;
    [[nodiscard]] std::vector<std::string> array_keys(std::string_view key) const;
    [[nodiscard]] bool has_valid_input() const noexcept { return input_valid_; }
    [[nodiscard]] std::map<std::string, std::string> all() const;

private:
    Method method_;
    RouteKind kind_;
    std::string path_;
    std::string body_;
    std::string raw_method_;
    std::string query_string_;
    std::map<std::string, std::string> headers_;
    std::map<std::string, std::string> route_parameters_;
    std::map<std::string, std::optional<std::string>> query_values_;
    std::map<std::string, std::optional<std::string>> body_values_;
    std::set<std::string> body_arrays_;
    std::set<std::string> body_objects_;
    std::map<std::string, std::vector<std::string>> body_array_values_;
    std::map<std::string, std::vector<std::string>> body_object_keys_;
    std::map<std::string, UploadedFile> files_;
    bool input_valid_ = true;
};

// Backward-compatible name for middleware and handlers that used RequestView.
using RequestView = Request;

// Returns the request active on the current HTTP worker thread. It is valid only
// while middleware or a route handler is executing.
[[nodiscard]] const Request& request();

namespace detail {
class RequestScope final {
public:
    explicit RequestScope(const Request& current) noexcept;
    ~RequestScope();
    RequestScope(const RequestScope&) = delete;
    RequestScope& operator=(const RequestScope&) = delete;
private:
    const Request* previous_;
};
} // namespace detail

} // namespace kotavel::http

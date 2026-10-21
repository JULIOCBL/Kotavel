#include <kotavel/http/controller.hpp>
#include <kotavel/http/form_request.hpp>

#include <iostream>
#include <stdexcept>
#include <string>

namespace {
void check(bool condition, const char* message) {
    if (!condition) throw std::runtime_error(message);
}

class StoreItemRequest final : public kotavel::http::FormRequest {
public:
    using FormRequest::FormRequest;
    kotavel::http::ValidationRules rules() const override {
        return {{"name", {"required", "string", "min:3"}},
                {"email", {"nullable", "email"}}};
    }
};

class DeniedRequest final : public kotavel::http::FormRequest {
public:
    using FormRequest::FormRequest;
    bool authorize() const override { return false; }
    kotavel::http::ValidationRules rules() const override { return {}; }
};

class NullableRequiredRequest final : public kotavel::http::FormRequest {
public:
    using FormRequest::FormRequest;
    kotavel::http::ValidationRules rules() const override {
        return {{"name", {"required", "nullable", "string"}},
                {"nickname", {"nullable", "string"}}};
    }
};

class NullableOptionalRequest final : public kotavel::http::FormRequest {
public:
    using FormRequest::FormRequest;
    kotavel::http::ValidationRules rules() const override {
        return {{"nickname", {"nullable", "string"}}};
    }
};

class RulesRequest : public kotavel::http::FormRequest {
public:
    RulesRequest(const kotavel::http::Request& request, kotavel::http::ValidationRules rules)
        : FormRequest(request), rules_(std::move(rules)) {}
    kotavel::http::ValidationRules rules() const override { return rules_; }
private:
    kotavel::http::ValidationRules rules_;
};

class CurrentPasswordRequest final : public RulesRequest {
public:
    using RulesRequest::RulesRequest;
    bool current_password_matches(std::string_view password, std::string_view guard) const override {
        return password == "correct-horse" && guard == "web";
    }
};

kotavel::http::Request json_request(std::string body) {
    return kotavel::http::Request(kotavel::http::RequestData{
        .method = kotavel::http::Method::post,
        .kind = kotavel::http::RouteKind::api,
        .path = "/api/validation",
        .body = std::move(body),
        .headers = {{"content-type", "application/json"}},
    });
}

class ItemController final : public kotavel::http::Controller {
public:
    kotavel::http::Response store(const StoreItemRequest& form) const {
        return kotavel::http::ApiResponse::success(form.validated(), 201);
    }
};
}

int main() {
    using namespace kotavel::http;

    RequestData data{
        .method = Method::post,
        .kind = RouteKind::api,
        .path = "/api/items/42",
        .body = R"({"name":"  Kotavel  ","profile":{"city":"  Mérida "},"tags":[" one ","two"]})",
        .raw_method = "POST",
        .query_string = "page=2&search=blue+tools&name=query-name&empty=%20%20",
        .headers = {{"Content-Type", "application/json; charset=utf-8"}, {"X-Trace", "abc"}},
        .route_parameters = {{"id", "42"}},
    };
    Request current(data);
    check(current.input("name") == "Kotavel", "JSON input should be trimmed and beat query values");
    check(current.query("search") == "blue tools", "query strings should be URL-decoded");
    check(current.input("page") == "2", "input should fall back to query values");
    check(current.has("empty") && !current.filled("empty"),
          "has() should detect a present blank field while filled() treats it as null");
    check(current.input("profile.city") == "Mérida" && current.input("tags.0") == "one",
          "nested JSON objects and arrays should support dot-notation access");
    check(current.route("id") == "42", "route parameters should be exposed by name");
    check(current.header("x-TRACE") == "abc", "header lookup should be case-insensitive");
    check(current.all().at("name") == "Kotavel", "all() should merge body values over query values");

    Request malformed(RequestData{.method = Method::post, .kind = RouteKind::api,
        .path = "/api/items", .body = R"({"name":)", .headers = {{"content-type", "application/json"}}});
    check(!malformed.has_valid_input(), "malformed JSON should be marked as invalid");

    StoreItemRequest valid(current);
    check(valid.validate() && valid.validated().at("name") == "Kotavel",
          "FormRequest should validate and return only sanitized allowed fields");

    Request empty_data(RequestData{.method = Method::post, .kind = RouteKind::api,
        .path = "/api/items", .body = R"({"name":"  "})",
        .headers = {{"content-type", "application/json"}}});
    NullableRequiredRequest empty_form(empty_data);
    check(!empty_form.validate() && empty_form.errors().size() == 1,
          "nullable must not bypass the required rule for blank input");

    Request optional_data(RequestData{.method = Method::post, .kind = RouteKind::api,
        .path = "/api/items", .body = R"({})",
        .headers = {{"content-type", "application/json"}}});
    NullableOptionalRequest optional_form(optional_data);
    check(optional_form.validate() && optional_form.validated().empty(),
          "missing optional nullable fields should pass and stay out of validated input");

    std::string rule_payload = R"({
      "name":"Ada", "username":"ada_7", "code":"A7", "ascii":"plain",
      "lower":"plain", "upper":"LOUD", "email":"ada@example.com",
      "site":"https://example.com/path", "local_site":"http://localhost",
      "ip":"127.0.0.1", "ip6":"2001:db8::1", "json_text":"{\"ok\":true}",
      "color":"#aabbcc", "amount":"12.50", "digits":"007", "password":"secret",
      "password_confirmation":"secret", "starts":"prefix-value", "ends":"value-suffix",
      "token":"550e8400-e29b-41d4-a716-446655440000", "ulid":"01ARZ3NDEKTSV4RRFFQ69G5FAV",
      "start":"2026-01-01", "end":"2026-02-01", "role":"admin",
      "roles":["admin","editor"], "rows":[{"code":"row_1"},{"code":"row_2"}],
      "alternative":"ada_7", "settings":{"theme":"dark","locale":"es"},
      "locale_date":"31/12/2026", "clock":"17:30:05", "zone":"UTC",
      "encoding_text":"Mérida", "terms":"yes",
      "decline":"no", "acceptance":"yes", "declined_status":"no", "choices":["red","blue"],
      "choice":"blue"
    })";
    Request rule_data = json_request(rule_payload);
    RulesRequest scalar_rules(rule_data, {
        {"name", {"required", "alpha"}}, {"username", {"alpha_dash"}},
        {"code", {"alpha_num"}}, {"ascii", {"ascii"}}, {"lower", {"lowercase"}},
        {"upper", {"uppercase"}}, {"email", {"email"}}, {"site", {"url"}},
        {"local_site", {"active_url"}}, {"ip", {"ip", "ipv4"}}, {"ip6", {"ipv6"}},
        {"json_text", {"json"}}, {"color", {"hex_color"}},
        {"amount", {"numeric", "decimal:2", "min:10", "max:20", "between:10,20"}},
        {"digits", {"integer", "digits:3", "min_digits:2", "max_digits:4", "digits_between:2,4"}},
        {"password", {"confirmed", "same:password_confirmation"}},
        {"starts", {"starts_with:prefix", "doesnt_end_with:wrong"}},
        {"ends", {"ends_with:suffix", "doesnt_start_with:wrong"}},
        {"token", {"uuid"}}, {"ulid", {"ulid"}}, {"start", {"date", "date_format:Y-m-d", "date_equals:2026-01-01"}},
        {"locale_date", {"date_format:d/m/Y, Y-m-d"}}, {"clock", {"date_format:H:i:s"}},
        {"zone", {"timezone"}},
        {"encoding_text", {"encoding:utf-8"}},
        {"end", {"date", "after:start", "after_or_equal:start", "before:2027-01-01",
                 "before_or_equal:2027-01-01"}},
        {"role", {"in:admin,editor", "not_in:guest", "enum:admin,editor"}},
        {"roles", {"array", "list", "contains:admin,editor", "distinct", "size:2"}},
        {"roles.*", {"alpha"}}, {"rows.*.code", {"alpha_dash"}},
        {"alternative", {"any_of:email;alpha_dash"}},
        {"settings", {"array", "array_keys:theme,locale", "required_array_keys:theme,locale",
                       "in_array_keys:locale", "size:2"}},
        {"choices", {"array", "contains:red,blue", "doesnt_contain:green", "distinct"}},
        {"choice", {"in_array:choices.*"}},
        {"acceptance", {"accepted_if:terms,yes"}}, {"declined_status", {"declined_if:decline,no"}},
    });
    const bool scalar_valid = scalar_rules.validate();
    if (!scalar_valid) for (const auto& error : scalar_rules.errors())
        std::cerr << error.field.value_or("?") << ": " << error.detail << '\n';
    check(scalar_valid, "Laravel-style scalar, date, numeric, and array rules should accept valid input");

    Request conditional_accept_data = json_request(R"({"terms":"yes","consent":"no"})");
    RulesRequest conditional_accept(conditional_accept_data, {
        {"consent", {"accepted_if:terms,yes"}}
    });
    check(!conditional_accept.validate() && conditional_accept.errors().size() == 1,
          "accepted_if must enforce acceptance only when the controlling field is accepted");

    Request ascii_encoding_data = json_request(R"({"value":"Mérida"})");
    RulesRequest ascii_encoding(ascii_encoding_data, {{"value", {"encoding:ascii"}}});
    check(!ascii_encoding.validate(), "ASCII encoding validation must reject non-ASCII text");

    Request bad_rules_data = json_request(R"({
      "slug":"bad slug", "amount":"nope", "role":"guest", "duplicate":["x","x"],
      "email":"invalid", "value":"wrong", "flag":"yes", "hidden":"present",
      "metadata":{"locale":"es"}
    })");
    RulesRequest invalid_rules(bad_rules_data, {
        {"slug", {"alpha_dash"}}, {"amount", {"numeric"}}, {"role", {"in:admin,editor"}},
        {"duplicate", {"distinct"}}, {"email", {"email"}}, {"value", {"regex:^ok$"}},
        {"flag", {"boolean"}}, {"hidden", {"missing"}},
        {"metadata", {"required_array_keys:source"}},
    });
    check(!invalid_rules.validate() && invalid_rules.errors().size() == 9,
          "invalid values should fail their respective rule families");

    Request conditional_data = json_request(R"({
      "mode":"card", "card_number":"123", "terms":"yes", "decline":"no",
      "contact":"email", "contact_address":"a@example.com", "email":"a@example.com",
      "without_a":"x", "require_with":"present", "require_without":"present",
      "require_without_all":"present", "present_field":"present", "terms_detail":"accepted",
      "decline_detail":"declined", "excluded":"drop"
    })");
    RulesRequest conditional(conditional_data, {
        {"card_number", {"required_if:mode,card"}},
        {"terms_detail", {"required_if_accepted:terms"}},
        {"decline_detail", {"required_if_declined:decline"}},
        {"contact_address", {"required_if:contact,email"}},
        {"require_with", {"required_with:email"}},
        {"require_without", {"required_without:absent"}},
        {"require_without_all", {"required_without_all:absent_a,absent_b"}},
        {"present_field", {"present_if:mode,card"}},
        {"missing_field", {"missing_unless:mode,card"}},
        {"prohibited_field", {"prohibited_if:mode,cash"}},
        {"excluded", {"exclude_if:mode,card"}},
    });
    const bool conditional_valid = conditional.validate();
    if (!conditional_valid) for (const auto& error : conditional.errors())
        std::cerr << error.field.value_or("?") << ": " << error.detail << '\n';
    check(conditional_valid && !conditional.validated().contains("excluded"),
          "conditional, presence, prohibited, and exclusion rules should behave as expected");

    Request password_data = json_request(R"({"password":"correct-horse"})");
    CurrentPasswordRequest password_request(password_data, {{"password", {"current_password:web"}}});
    check(password_request.validate(), "current_password should delegate to the host authentication provider");

    std::string png("\x89PNG\r\n\x1a\n", 8);
    png += std::string("\0\0\0\rIHDR\0\0\0\x01\0\0\0\x01", 16);
    png += std::string(8, '\0');
    const std::string multipart = "--unit-boundary\r\n"
        "Content-Disposition: form-data; name=\"upload\"; filename=\"pixel.png\"\r\n"
        "Content-Type: application/octet-stream\r\n\r\n" + png +
        "\r\n--unit-boundary--\r\n";
    Request file_data(RequestData{.method = Method::post, .kind = RouteKind::api,
        .path = "/api/upload", .body = multipart,
        .headers = {{"content-type", "multipart/form-data; boundary=unit-boundary"}}});
    RulesRequest file_rules(file_data, {{"upload", {"required", "file", "image", "mimes:png",
        "mimetypes:image/png", "extensions:png", "dimensions:width=1,height=1", "max:2"}}});
    check(file_rules.validate() && file_rules.validated().at("upload") == "pixel.png",
          "multipart uploads should support file, MIME, extension, dimensions and size rules");

    Request invalid_data(RequestData{.method = Method::post, .kind = RouteKind::api,
        .path = "/api/items", .body = R"({"name":" x ","extra":"ignored"})",
        .headers = {{"content-type", "application/json"}}});
    StoreItemRequest invalid(invalid_data);
    check(!invalid.validate() && invalid.errors().size() == 1,
          "FormRequest should report field validation errors");
    const auto validation_response = invalid.error_response();
    check(validation_response.status == 422 &&
          validation_response.body.find(R"("source":{"field":"name"})") != std::string::npos,
          "API validation failures should use the standard error envelope and identify fields");

    DeniedRequest denied(invalid_data);
    check(!denied.validate() && denied.error_response().status == 403,
          "FormRequest authorization failures should return 403");

    ItemController controller;
    const auto action = Controller::action(controller, &ItemController::store);
    {
        detail::RequestScope scope(current);
        check(request().path() == "/api/items/42", "request() should expose the active request in its scope");
        const auto created = action(current);
        check(created.status == 201 && created.body == R"({"data":{"name":"Kotavel"}})",
              "Controller actions should validate FormRequest before invoking the controller");
    }

    bool helper_rejected_outside_scope = false;
    try { (void)request(); }
    catch (const std::logic_error&) { helper_rejected_outside_scope = true; }
    check(helper_rejected_outside_scope,
          "request() should reject access outside request handling");

    std::cout << "HTTP request and FormRequest tests passed\n";
}

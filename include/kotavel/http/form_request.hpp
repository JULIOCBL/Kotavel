#pragma once

#include <map>
#include <string>
#include <vector>

#include <kotavel/http/api_response.hpp>
#include <kotavel/http/request.hpp>

namespace kotavel::http {

// Laravel-style validation rules: {"email": {"required", "email"}}.
using ValidationRules = std::map<std::string, std::vector<std::string>>;

class FormRequest : public Request {
public:
    explicit FormRequest(const Request& request) : Request(request) {}
    virtual ~FormRequest() = default;

    [[nodiscard]] virtual bool authorize() const { return true; }
    [[nodiscard]] virtual ValidationRules rules() const = 0;
    // Override when the host application has an authenticated user/password provider.
    [[nodiscard]] virtual bool current_password_matches(
        std::string_view password, std::string_view guard) const {
        (void)password;
        (void)guard;
        return false;
    }

    // Validates and sanitizes input. On success, validated() contains only fields
    // declared in rules(); on failure, error_response() returns 403 or 422.
    [[nodiscard]] bool validate();
    [[nodiscard]] const std::map<std::string, std::string>& validated() const;
    [[nodiscard]] const std::vector<ApiError>& errors() const noexcept { return errors_; }
    [[nodiscard]] Response error_response() const;

private:
    bool validation_ran_ = false;
    bool authorized_ = true;
    std::map<std::string, std::string> validated_;
    std::vector<ApiError> errors_;
};

} // namespace kotavel::http

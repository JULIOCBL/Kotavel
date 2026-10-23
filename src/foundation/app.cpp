#include <kotavel/foundation/app.hpp>

#include <optional>
#include <stdexcept>
#include <utility>

namespace kotavel {
namespace {
thread_local std::optional<std::string> active_locale;

void validate_locale(std::string_view locale) {
    if (locale.empty() || locale.size() > 64)
        throw std::invalid_argument("Locale must contain between 1 and 64 characters");

    bool segment_start = true;
    std::size_t segment_length = 0;
    for (const unsigned char character : locale) {
        if (character == '-' || character == '_') {
            if (segment_start || segment_length == 0 || segment_length > 8)
                throw std::invalid_argument("Locale must be a valid language tag");
            segment_start = true;
            segment_length = 0;
            continue;
        }
        const bool alphanumeric = (character >= 'a' && character <= 'z') ||
            (character >= 'A' && character <= 'Z') ||
            (character >= '0' && character <= '9');
        if (!alphanumeric)
            throw std::invalid_argument("Locale must be a valid language tag");
        segment_start = false;
        ++segment_length;
    }
    if (segment_start || segment_length == 0 || segment_length > 8)
        throw std::invalid_argument("Locale must be a valid language tag");
}
}

void App::setLocale(std::string_view locale) const {
    validate_locale(locale);
    active_locale = std::string(locale);
}

std::string App::currentLocale() const {
    return active_locale.value_or("en");
}

bool App::isLocale(std::string_view locale) const {
    return currentLocale() == locale;
}

App& app() noexcept {
    static App instance;
    return instance;
}

detail::LocaleScope::LocaleScope(std::string_view default_locale) {
    validate_locale(default_locale);
    had_previous_ = active_locale.has_value();
    if (had_previous_) previous_ = *active_locale;
    active_locale = std::string(default_locale);
}

detail::LocaleScope::~LocaleScope() {
    if (had_previous_) active_locale = std::move(previous_);
    else active_locale.reset();
}

} // namespace kotavel

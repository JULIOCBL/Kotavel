#pragma once

#include <string>
#include <string_view>

namespace kotavel {

// Lightweight application facade. Locale overrides belong to the current
// execution context so one HTTP request cannot change another request's locale.
class App final {
public:
    void setLocale(std::string_view locale) const;
    [[nodiscard]] std::string currentLocale() const;
    [[nodiscard]] bool isLocale(std::string_view locale) const;
};

// Laravel-style access to the current application facade:
// app().setLocale("es");
[[nodiscard]] App& app() noexcept;

namespace detail {
class LocaleScope final {
public:
    explicit LocaleScope(std::string_view default_locale = "en");
    ~LocaleScope();
    LocaleScope(const LocaleScope&) = delete;
    LocaleScope& operator=(const LocaleScope&) = delete;
private:
    bool had_previous_ = false;
    std::string previous_;
};
} // namespace detail

} // namespace kotavel

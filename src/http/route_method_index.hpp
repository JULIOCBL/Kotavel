#pragma once

#include <kotavel/http/router.hpp>
#include <cstdint>
#include <optional>
#include <unordered_map>

namespace kotavel::http::detail {
inline bool route_path_matches(std::string_view pattern, std::string_view path) {
    std::size_t pattern_start = 0;
    std::size_t path_start = 0;
    while (pattern_start < pattern.size() || path_start < path.size()) {
        if (pattern_start == pattern.size() || path_start == path.size())
            return pattern_start == pattern.size() && path_start == path.size();

        const auto pattern_end = pattern.find('/', pattern_start);
        const auto path_end = path.find('/', path_start);
        const auto pattern_segment = pattern.substr(
            pattern_start, pattern_end == std::string_view::npos
                ? pattern.size() - pattern_start : pattern_end - pattern_start);
        const auto path_segment = path.substr(
            path_start, path_end == std::string_view::npos
                ? path.size() - path_start : path_end - path_start);

        const bool parameter = pattern_segment.size() >= 2 &&
            pattern_segment.front() == '{' && pattern_segment.back() == '}';
        if (!parameter && pattern_segment != path_segment) return false;
        if (parameter && path_segment.empty()) return false;

        if (pattern_end == std::string_view::npos || path_end == std::string_view::npos)
            return pattern_end == path_end;
        pattern_start = pattern_end + 1;
        path_start = path_end + 1;
    }
    return true;
}


// Built once after routes are sealed. Literal paths use a hash lookup; only
// parameterized patterns require segment matching. Read-only across workers.
class RouteMethodIndex final {
    struct PathHash {
        using is_transparent = void;
        std::size_t operator()(std::string_view path) const noexcept {
            return std::hash<std::string_view>{}(path);
        }
    };
    std::unordered_map<std::string, unsigned, PathHash, std::equal_to<>> literals_;
    std::vector<std::pair<std::string, unsigned>> patterns_;

public:
    explicit RouteMethodIndex(const std::vector<RouteDefinition>& routes) {
        literals_.reserve(routes.size());
        for (const auto& route : routes) {
            const auto mask = 1u << static_cast<unsigned>(route.method);
            if (route.path.find('{') == std::string::npos) literals_[route.path] |= mask;
            else patterns_.emplace_back(route.path, mask);
        }
    }

    bool rejects(std::string_view path, std::optional<Method> method) const {
        const auto mask = method ? 1u << static_cast<unsigned>(*method) : 0u;
        bool registered = false;
        if (const auto found = literals_.find(path); found != literals_.end()) {
            registered = true;
            if (found->second & mask) return false;
        }
        for (const auto& [pattern, methods] : patterns_) {
            if (!route_path_matches(pattern, path)) continue;
            registered = true;
            if (methods & mask) return false;
        }
        return registered;
    }
};
} // namespace kotavel::http::detail

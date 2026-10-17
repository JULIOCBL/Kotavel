#include <kotavel/desktop/deep_link_router.hpp>

#include <stdexcept>
#include <utility>

namespace kotavel::desktop {
namespace {
bool valid_name(std::string_view value) {
    if (value.empty()) return false;
    for (const unsigned char ch : value) {
        if (!((ch >= 'a' && ch <= 'z') || (ch >= 'A' && ch <= 'Z') ||
              (ch >= '0' && ch <= '9') || ch == '-' || ch == '_' || ch == '/'))
            return false;
    }
    return value.front() != '/' && value.back() != '/' && value.find("//") == std::string_view::npos;
}
}

DeepLinkRouter::DeepLinkRouter(std::string scheme) : scheme_(std::move(scheme)) {
    if (!valid_name(scheme_) || scheme_.find('/') != std::string::npos)
        throw std::invalid_argument("Deep-link scheme must be a non-empty URI token");
}

void DeepLinkRouter::screen(std::string_view route, DeepLinkHandler handler) {
    if (!valid_name(route)) throw std::invalid_argument("Deep-link screen route is invalid");
    if (!handler) throw std::invalid_argument("Deep-link screen handler cannot be empty");
    if (!screens_.emplace(std::string(route), std::move(handler)).second)
        throw std::logic_error("Duplicate desktop deep-link route: " + std::string(route));
}

DeepLinkResult DeepLinkRouter::dispatch(std::string_view uri) const {
    const std::string prefix = scheme_ + "://";
    if (!uri.starts_with(prefix)) return DeepLinkResult::invalid_uri;
    const auto route = uri.substr(prefix.size());
    if (!valid_name(route)) return DeepLinkResult::invalid_uri;
    const auto found = screens_.find(std::string(route));
    if (found == screens_.end()) return DeepLinkResult::unknown_route;
    found->second();
    return DeepLinkResult::dispatched;
}

} // namespace kotavel::desktop

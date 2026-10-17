#pragma once

#include <functional>
#include <string>
#include <string_view>
#include <unordered_map>

namespace kotavel::desktop {

enum class DeepLinkResult { dispatched, invalid_uri, unknown_route };
using DeepLinkHandler = std::function<void()>;

// Traduce un URI entregado por el sistema operativo al nombre de una Screen.
class DeepLinkRouter final {
public:
    explicit DeepLinkRouter(std::string scheme);
    void screen(std::string_view route, DeepLinkHandler handler);
    [[nodiscard]] DeepLinkResult dispatch(std::string_view uri) const;

private:
    std::string scheme_;
    std::unordered_map<std::string, DeepLinkHandler> screens_;
};

} // namespace kotavel::desktop

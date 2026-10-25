#pragma once

#include <memory>
#include <cstdint>
#include <string>

#include "rust/cxx.h"

namespace kotavel::desktop {

struct DeepLinkDispatcher;

class QtHost final {
public:
    explicit QtHost(rust::Vec<rust::String> arguments);
    ~QtHost();

    QtHost(const QtHost&) = delete;
    QtHost& operator=(const QtHost&) = delete;

    void show(rust::Str id, rust::Str title, std::uint32_t width,
              std::uint32_t height, rust::Str source, rust::Str icon) const;
    void close(rust::Str id) const noexcept;
    [[nodiscard]] bool is_visible(rust::Str id) const noexcept;
    [[nodiscard]] bool has_visible_window() const noexcept;
    [[nodiscard]] int exec(DeepLinkDispatcher& deep_link) const;
    void quit() const noexcept;

private:
    struct State;
    std::unique_ptr<State> state_;
};

[[nodiscard]] std::unique_ptr<QtHost> create_qt_host(rust::Vec<rust::String> arguments);

} // namespace kotavel::desktop

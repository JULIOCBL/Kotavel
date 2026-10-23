#pragma once

#include <chrono>
#include <cstdint>
#include <memory>
#include <string_view>

namespace kotavel::platform::detail {

class LocalMetricsConnection {
public:
    virtual ~LocalMetricsConnection() = default;
    [[nodiscard]] virtual bool send(std::string_view payload) noexcept = 0;
};

class LocalMetricsServer {
public:
    virtual ~LocalMetricsServer() = default;
    [[nodiscard]] virtual std::uint16_t start_loopback() = 0;
    [[nodiscard]] virtual std::unique_ptr<LocalMetricsConnection>
    accept_for(std::chrono::milliseconds timeout) noexcept = 0;
    virtual void stop() noexcept = 0;
};

[[nodiscard]] std::unique_ptr<LocalMetricsServer> make_local_metrics_server();
[[nodiscard]] std::uint64_t current_process_id() noexcept;
// Returns false only when the PID is known to have exited; uncertain states
// are treated as live so endpoint cleanup never removes another process's file.
[[nodiscard]] bool process_is_running(std::uint64_t process_id) noexcept;

} // namespace kotavel::platform::detail

#include "../metrics_stream_transport.hpp"
#include "../socket_signal.hpp"

#include <arpa/inet.h>
#include <cerrno>
#include <netinet/in.h>
#include <sys/socket.h>
#include <unistd.h>

#include <algorithm>
#include <string>
#include <stdexcept>

namespace kotavel::platform::detail {
namespace {
class PosixMetricsConnection final : public LocalMetricsConnection {
public:
    explicit PosixMetricsConnection(int socket) : socket_(socket) {
        const timeval timeout{0, 100000};
        (void)setsockopt(socket_, SOL_SOCKET, SO_SNDTIMEO, &timeout, sizeof(timeout));
        configure_socket_signal_handling(socket_);
    }
    ~PosixMetricsConnection() override { if (socket_ >= 0) ::close(socket_); }
    bool send(std::string_view payload) noexcept override {
        std::size_t sent = 0;
        while (sent < payload.size()) {
            const auto count = send_without_sigpipe(
                socket_, payload.data() + sent, payload.size() - sent);
            if (count <= 0) {
                if (errno == EINTR) continue;
                return false;
            }
            sent += static_cast<std::size_t>(count);
        }
        return true;
    }
private:
    int socket_ = -1;
};

class PosixMetricsServer final : public LocalMetricsServer {
public:
    ~PosixMetricsServer() override { stop(); }

    std::uint16_t start_loopback() override {
        if (listener_ >= 0) throw std::logic_error("Metrics server already started");
        listener_ = ::socket(AF_INET, SOCK_STREAM, 0);
        if (listener_ < 0) fail("Could not create local metrics listener");
        sockaddr_in address{};
        address.sin_family = AF_INET;
        address.sin_addr.s_addr = htonl(INADDR_LOOPBACK);
        address.sin_port = 0;
        if (::bind(listener_, reinterpret_cast<const sockaddr*>(&address), sizeof(address)) != 0)
            fail("Could not bind local metrics listener");
        if (::listen(listener_, 1) != 0) fail("Could not listen on local metrics socket");
        socklen_t address_size = sizeof(address);
        if (getsockname(listener_, reinterpret_cast<sockaddr*>(&address), &address_size) != 0)
            fail("Could not read local metrics port");
        return ntohs(address.sin_port);
    }

    std::unique_ptr<LocalMetricsConnection> accept_for(
        std::chrono::milliseconds timeout) noexcept override {
        if (listener_ < 0) return {};
        fd_set readers;
        FD_ZERO(&readers);
        FD_SET(listener_, &readers);
        const auto micros = std::chrono::duration_cast<std::chrono::microseconds>(timeout);
        timeval wait{};
        wait.tv_sec = static_cast<decltype(wait.tv_sec)>(micros.count() / 1'000'000);
        wait.tv_usec = static_cast<decltype(wait.tv_usec)>(micros.count() % 1'000'000);
        const int ready = select(listener_ + 1, &readers, nullptr, nullptr, &wait);
        if (ready <= 0 || !FD_ISSET(listener_, &readers)) return {};
        sockaddr_in peer{};
        socklen_t peer_size = sizeof(peer);
        const int connected = ::accept(listener_, reinterpret_cast<sockaddr*>(&peer), &peer_size);
        if (connected < 0) return {};
        try { return std::make_unique<PosixMetricsConnection>(connected); }
        catch (...) { ::close(connected); return {}; }
    }

    void stop() noexcept override {
        if (listener_ >= 0) {
            ::close(listener_);
            listener_ = -1;
        }
    }
private:
    [[noreturn]] void fail(const char* message) {
        const auto error = errno;
        stop();
        throw std::runtime_error(std::string(message) + ": " + std::to_string(error));
    }
    int listener_ = -1;
};
}

std::unique_ptr<LocalMetricsServer> make_local_metrics_server() {
    return std::make_unique<PosixMetricsServer>();
}
std::uint64_t current_process_id() noexcept { return static_cast<std::uint64_t>(getpid()); }
} // namespace kotavel::platform::detail

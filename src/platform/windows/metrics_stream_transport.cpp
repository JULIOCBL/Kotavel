#include "../metrics_stream_transport.hpp"

#define NOMINMAX
#include <winsock2.h>
#include <ws2tcpip.h>
#include <windows.h>

#include <algorithm>
#include <limits>
#include <string>
#include <stdexcept>

namespace kotavel::platform::detail {
namespace {
class WindowsMetricsConnection final : public LocalMetricsConnection {
public:
    explicit WindowsMetricsConnection(SOCKET socket) : socket_(socket) {
        const DWORD timeout = 100;
        (void)setsockopt(socket_, SOL_SOCKET, SO_SNDTIMEO,
                         reinterpret_cast<const char*>(&timeout), sizeof(timeout));
    }
    ~WindowsMetricsConnection() override {
        if (socket_ != INVALID_SOCKET) closesocket(socket_);
    }
    bool send(std::string_view payload) noexcept override {
        std::size_t sent = 0;
        while (sent < payload.size()) {
            const auto remaining = (std::min)(payload.size() - sent,
                static_cast<std::size_t>(std::numeric_limits<int>::max()));
            const int count = ::send(socket_, payload.data() + sent,
                                     static_cast<int>(remaining), 0);
            if (count == SOCKET_ERROR) {
                if (WSAGetLastError() == WSAEINTR) continue;
                return false;
            }
            if (count == 0) return false;
            sent += static_cast<std::size_t>(count);
        }
        return true;
    }
private:
    SOCKET socket_ = INVALID_SOCKET;
};

class WindowsMetricsServer final : public LocalMetricsServer {
public:
    ~WindowsMetricsServer() override { stop(); }

    std::uint16_t start_loopback() override {
        if (listener_ != INVALID_SOCKET) throw std::logic_error("Metrics server already started");
        WSADATA data{};
        if (WSAStartup(MAKEWORD(2, 2), &data) != 0)
            throw std::runtime_error("Could not initialize local metrics transport");
        winsock_started_ = true;
        listener_ = ::socket(AF_INET, SOCK_STREAM, IPPROTO_TCP);
        if (listener_ == INVALID_SOCKET) fail("Could not create local metrics listener");

        sockaddr_in address{};
        address.sin_family = AF_INET;
        address.sin_addr.s_addr = htonl(INADDR_LOOPBACK);
        address.sin_port = 0;
        if (::bind(listener_, reinterpret_cast<const sockaddr*>(&address), sizeof(address)) != 0)
            fail("Could not bind local metrics listener");
        if (::listen(listener_, 1) != 0) fail("Could not listen on local metrics socket");
        int address_size = sizeof(address);
        if (getsockname(listener_, reinterpret_cast<sockaddr*>(&address), &address_size) != 0)
            fail("Could not read local metrics port");
        return ntohs(address.sin_port);
    }

    std::unique_ptr<LocalMetricsConnection> accept_for(
        std::chrono::milliseconds timeout) noexcept override {
        if (listener_ == INVALID_SOCKET) return {};
        fd_set readers;
        FD_ZERO(&readers);
        FD_SET(listener_, &readers);
        const auto micros = std::chrono::duration_cast<std::chrono::microseconds>(timeout);
        timeval wait{static_cast<long>(micros.count() / 1'000'000),
                     static_cast<long>(micros.count() % 1'000'000)};
        const int ready = select(0, &readers, nullptr, nullptr, &wait);
        if (ready <= 0 || !FD_ISSET(listener_, &readers)) return {};
        sockaddr_in peer{};
        int peer_size = sizeof(peer);
        const SOCKET connected = ::accept(listener_, reinterpret_cast<sockaddr*>(&peer), &peer_size);
        if (connected == INVALID_SOCKET) return {};
        try { return std::make_unique<WindowsMetricsConnection>(connected); }
        catch (...) { closesocket(connected); return {}; }
    }

    void stop() noexcept override {
        if (listener_ != INVALID_SOCKET) {
            closesocket(listener_);
            listener_ = INVALID_SOCKET;
        }
        if (winsock_started_) {
            WSACleanup();
            winsock_started_ = false;
        }
    }
private:
    [[noreturn]] void fail(const char* message) {
        const auto error = WSAGetLastError();
        stop();
        throw std::runtime_error(std::string(message) + ": " + std::to_string(error));
    }
    SOCKET listener_ = INVALID_SOCKET;
    bool winsock_started_ = false;
};
}

std::unique_ptr<LocalMetricsServer> make_local_metrics_server() {
    return std::make_unique<WindowsMetricsServer>();
}
std::uint64_t current_process_id() noexcept { return GetCurrentProcessId(); }
} // namespace kotavel::platform::detail

#include <kotavel/metrics/metrics.hpp>

#include <atomic>
#include <cmath>
#include <fstream>
#include <iomanip>
#include <locale>
#include <mutex>
#include <sstream>
#include <stdexcept>
#include <string>
#include <thread>
#include <utility>

#ifdef _WIN32
#define NOMINMAX
#include <winsock2.h>
#include <ws2tcpip.h>
#include <windows.h>
#else
#include <arpa/inet.h>
#include <cerrno>
#include <netinet/in.h>
#include <sys/socket.h>
#include <unistd.h>
#endif

namespace kotavel::metrics {
namespace {
#ifdef _WIN32
using Socket = SOCKET;
constexpr Socket invalid_socket = INVALID_SOCKET;
void close_socket(Socket socket) noexcept { if (socket != invalid_socket) closesocket(socket); }
int select_socket(Socket socket, fd_set* readers, timeval* timeout) {
    return select(0, readers, nullptr, nullptr, timeout);
}
int socket_error() noexcept { return WSAGetLastError(); }
bool interrupted(int error) noexcept { return error == WSAEINTR; }
unsigned long current_process_id() noexcept { return GetCurrentProcessId(); }
#else
using Socket = int;
constexpr Socket invalid_socket = -1;
void close_socket(Socket socket) noexcept { if (socket != invalid_socket) ::close(socket); }
int select_socket(Socket socket, fd_set* readers, timeval* timeout) {
    return select(socket + 1, readers, nullptr, nullptr, timeout);
}
int socket_error() noexcept { return errno; }
bool interrupted(int error) noexcept { return error == EINTR; }
unsigned long current_process_id() noexcept { return static_cast<unsigned long>(getpid()); }
#endif

bool send_all(Socket socket, const std::string& message) noexcept {
    std::size_t sent = 0;
    while (sent < message.size()) {
#ifdef _WIN32
        const int count = ::send(socket, message.data() + sent,
                                 static_cast<int>(message.size() - sent), 0);
#else
#ifdef MSG_NOSIGNAL
        const int count = ::send(socket, message.data() + sent, message.size() - sent, MSG_NOSIGNAL);
#else
        const int count = ::send(socket, message.data() + sent, message.size() - sent, 0);
#endif
#endif
        if (count <= 0) {
            const int error = socket_error();
            if (interrupted(error)) continue;
            return false;
        }
        sent += static_cast<std::size_t>(count);
    }
    return true;
}

void set_send_timeout(Socket socket) noexcept {
#ifdef _WIN32
    const DWORD timeout = 100;
    (void)setsockopt(socket, SOL_SOCKET, SO_SNDTIMEO,
                     reinterpret_cast<const char*>(&timeout), sizeof(timeout));
#else
    const timeval timeout{0, 100000};
    (void)setsockopt(socket, SOL_SOCKET, SO_SNDTIMEO, &timeout, sizeof(timeout));
#if defined(__APPLE__) && defined(SO_NOSIGPIPE)
    const int enabled = 1;
    (void)setsockopt(socket, SOL_SOCKET, SO_NOSIGPIPE, &enabled, sizeof(enabled));
#endif
#endif
}
}

struct MetricsStream::Impl {
    std::filesystem::path endpoint;
    Socket listener = invalid_socket;
    std::thread worker;
    std::atomic<bool> stopping{true};
    std::atomic<bool> active{false};
    std::mutex sample_mutex;
    std::string startup_message;
    std::string latest_sample;
    std::uint64_t generation = 0;
#ifdef _WIN32
    bool winsock_started = false;
#endif

    void serve() noexcept {
        Socket client = invalid_socket;
        std::uint64_t sent_generation = 0;
        bool sent_startup = false;
        try {
            while (!stopping.load(std::memory_order_acquire)) {
                fd_set readers;
                FD_ZERO(&readers);
                FD_SET(listener, &readers);
                timeval timeout{0, 100000};
                const int ready = select_socket(listener, &readers, &timeout);
                if (ready < 0) {
                    if (interrupted(socket_error())) continue;
                    break;
                }
                if (ready > 0 && FD_ISSET(listener, &readers)) {
                    sockaddr_in peer{};
#ifdef _WIN32
                    int peer_size = sizeof(peer);
#else
                    socklen_t peer_size = sizeof(peer);
#endif
                    const Socket connected = accept(listener, reinterpret_cast<sockaddr*>(&peer), &peer_size);
                    if (connected != invalid_socket) {
                        close_socket(client);
                        client = connected;
                        set_send_timeout(client);
                        sent_generation = 0;
                        sent_startup = false;
                    }
                }
                if (client == invalid_socket) continue;

                std::string payload;
                std::uint64_t current_generation = 0;
                {
                    std::lock_guard lock(sample_mutex);
                    current_generation = generation;
                    if (!sent_startup) payload = startup_message;
                }
                if (!payload.empty()) {
                    if (!send_all(client, payload)) {
                        close_socket(client);
                        client = invalid_socket;
                    } else {
                        sent_startup = true;
                    }
                }
                if (client == invalid_socket) continue;

                {
                    std::lock_guard lock(sample_mutex);
                    current_generation = generation;
                    if (current_generation != sent_generation) payload = latest_sample;
                    else payload.clear();
                }
                if (!payload.empty()) {
                    if (!send_all(client, payload)) {
                        close_socket(client);
                        client = invalid_socket;
                    } else {
                        sent_generation = current_generation;
                    }
                }
            }
        } catch (...) {
            // Allocation or transport failures only stop the optional metrics stream.
        }
        close_socket(client);
        active.store(false, std::memory_order_release);
    }
};

MetricsStream::MetricsStream() : impl_(std::make_unique<Impl>()) {}

MetricsStream::~MetricsStream() { stop(); }

void MetricsStream::start(const std::filesystem::path& project_root) {
    if (impl_->active.load(std::memory_order_acquire) || impl_->worker.joinable())
        throw std::logic_error("Metrics stream is already running");

    const auto directory = project_root / "storage" / "framework" / "metrics";
    std::filesystem::create_directories(directory);
    const auto process_id = current_process_id();
    impl_->endpoint = directory / ("kotavel-" + std::to_string(process_id) + ".endpoint");

#ifdef _WIN32
    WSADATA data{};
    if (WSAStartup(MAKEWORD(2, 2), &data) != 0)
        throw std::runtime_error("Could not initialize local metrics transport");
    impl_->winsock_started = true;
#endif
    impl_->listener = ::socket(AF_INET, SOCK_STREAM, IPPROTO_TCP);
    if (impl_->listener == invalid_socket) {
        stop();
        throw std::runtime_error("Could not create local metrics listener");
    }

    sockaddr_in address{};
    address.sin_family = AF_INET;
    address.sin_addr.s_addr = htonl(INADDR_LOOPBACK);
    address.sin_port = 0;
    if (bind(impl_->listener, reinterpret_cast<const sockaddr*>(&address), sizeof(address)) != 0) {
        const int error = socket_error();
        stop();
        throw std::runtime_error("Could not bind local metrics listener: " + std::to_string(error));
    }
    if (listen(impl_->listener, 1) != 0) {
        const int error = socket_error();
        stop();
        throw std::runtime_error("Could not listen on local metrics socket: " + std::to_string(error));
    }
#ifdef _WIN32
    int address_size = sizeof(address);
#else
    socklen_t address_size = sizeof(address);
#endif
    if (getsockname(impl_->listener, reinterpret_cast<sockaddr*>(&address), &address_size) != 0) {
        stop();
        throw std::runtime_error("Could not read local metrics port");
    }

    {
        std::ofstream descriptor(impl_->endpoint, std::ios::trunc);
        if (!descriptor || !(descriptor << ntohs(address.sin_port) << '\n')) {
            stop();
            throw std::runtime_error("Could not publish local metrics endpoint");
        }
    }
    {
        std::lock_guard lock(impl_->sample_mutex);
        impl_->startup_message.clear();
        impl_->latest_sample.clear();
        impl_->generation = 0;
    }
    impl_->stopping.store(false, std::memory_order_release);
    try {
        impl_->worker = std::thread([state = impl_.get()] { state->serve(); });
    } catch (...) {
        stop();
        throw;
    }
    impl_->active.store(true, std::memory_order_release);
}

void MetricsStream::publish(const MetricsSample& sample) noexcept {
    try {
        std::ostringstream message;
        message.imbue(std::locale::classic());
        message << std::fixed << std::setprecision(1)
                << "{\"type\":\"sample\",\"cpu_percent\":";
        if (sample.process_cpu_percent && std::isfinite(*sample.process_cpu_percent) &&
            *sample.process_cpu_percent >= 0.0)
            message << *sample.process_cpu_percent;
        else message << "null";
        message << ",\"resident_memory_bytes\":" << sample.resident_memory_bytes << "}\n";
        std::lock_guard lock(impl_->sample_mutex);
        impl_->latest_sample = std::move(message).str();
        ++impl_->generation;
    } catch (...) {
        // A metrics transport failure must never interrupt the application.
    }
}

void MetricsStream::publish_startup(const StartupMetrics& metrics) noexcept {
    try {
        const auto milliseconds = [](std::chrono::nanoseconds value) {
            return std::chrono::duration<double, std::milli>(value).count();
        };
        std::ostringstream message;
        message.imbue(std::locale::classic());
        message << std::fixed << std::setprecision(2)
                << "{\"type\":\"startup\",\"environment_ms\":" << milliseconds(metrics.environment)
                << ",\"configuration_ms\":" << milliseconds(metrics.configuration)
                << ",\"settings_ms\":" << milliseconds(metrics.settings)
                << ",\"application_boot_ms\":" << milliseconds(metrics.application_boot)
                << ",\"total_ms\":" << milliseconds(metrics.total) << "}\n";
        std::lock_guard lock(impl_->sample_mutex);
        impl_->startup_message = std::move(message).str();
    } catch (...) {
        // A metrics transport failure must never interrupt the application.
    }
}

void MetricsStream::stop() noexcept {
    if (!impl_) return;
    impl_->stopping.store(true, std::memory_order_release);
    if (impl_->worker.joinable()) impl_->worker.join();
    close_socket(impl_->listener);
    impl_->listener = invalid_socket;
    if (!impl_->endpoint.empty()) {
        std::error_code error;
        std::filesystem::remove(impl_->endpoint, error);
        impl_->endpoint.clear();
    }
#ifdef _WIN32
    if (impl_->winsock_started) {
        WSACleanup();
        impl_->winsock_started = false;
    }
#endif
    impl_->active.store(false, std::memory_order_release);
}

bool MetricsStream::running() const noexcept {
    return impl_ && impl_->active.load(std::memory_order_acquire);
}

} // namespace kotavel::metrics

#include <kotavel/metrics/performance.hpp>

#include <chrono>
#include <filesystem>
#include <iostream>
#include <thread>

int main(int argc, char** argv) {
    if (argc != 2) return 2;
    try {
        kotavel::metrics::PerformanceStream stream;
        stream.start(std::filesystem::path(argv[1]));
        stream.publish_startup({std::chrono::milliseconds(1), std::chrono::milliseconds(2),
                                std::chrono::milliseconds(3), std::chrono::milliseconds(4),
                                std::chrono::milliseconds(10)});
        stream.publish({std::chrono::steady_clock::now(), 12.5, 4 * 1024 * 1024});
        std::cout << "ready\n" << std::flush;
        // Keep the endpoint available long enough for the platform-independent test client.
        std::this_thread::sleep_for(std::chrono::seconds(2));
        stream.stop();
        return 0;
    } catch (const std::exception& error) {
        std::cerr << error.what() << '\n';
        return 1;
    }
}

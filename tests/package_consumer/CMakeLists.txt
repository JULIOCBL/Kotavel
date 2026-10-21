cmake_minimum_required(VERSION 3.25)
project(KotavelBinaryConsumer LANGUAGES CXX)

find_package(Kotavel CONFIG REQUIRED)
add_executable(kotavel_binary_consumer main.cpp)
target_link_libraries(kotavel_binary_consumer PRIVATE Kotavel::Core)

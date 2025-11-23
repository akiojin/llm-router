cmake_minimum_required(VERSION 3.20)
project(ollama-node-cpp VERSION 1.0.0 LANGUAGES CXX C)

# Set C++ standard
set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)

# Export compile commands for IDE support
set(CMAKE_EXPORT_COMPILE_COMMANDS ON)

# Build type
if(NOT CMAKE_BUILD_TYPE)
    set(CMAKE_BUILD_TYPE Release)
endif()

# Compiler flags
if(CMAKE_CXX_COMPILER_ID MATCHES "GNU|Clang")
    add_compile_options(-Wall -Wextra -Wpedantic)
    if(CMAKE_BUILD_TYPE STREQUAL "Debug")
        add_compile_options(-g -O0)
    else()
        add_compile_options(-O3 -march=native)
    endif()
endif()

# Options
option(BUILD_TESTS "Build tests" ON)
option(BUILD_WITH_CUDA "Build with CUDA support" OFF)
option(BUILD_WITH_METAL "Build with Metal support (macOS)" OFF)
option(BUILD_WITH_ROCM "Build with ROCm support" OFF)
option(STATIC_BUILD "Build statically linked executable" OFF)

# ggml/llama.cpp の ARM ネイティブ最適化は、実行環境の命令対応を誤検出すると
# ビルドエラーになるため、ポータブル設定に固定する。
set(GGML_NATIVE OFF CACHE BOOL "Disable native cpu flags for portability" FORCE)
set(GGML_CPU_ARM_ARCH "armv8-a" CACHE STRING "Baseline ARM arch" FORCE)
set(GGML_CPU_ALL_VARIANTS OFF CACHE BOOL "Single ARM backend variant" FORCE)
set(GGML_INTERNAL_FP16_VECTOR_ARITHMETIC OFF CACHE BOOL "Force disable FP16 vector arithmetic" FORCE)
set(GGML_INTERNAL_MATMUL_INT8 OFF CACHE BOOL "Force disable i8mm" FORCE)
set(GGML_INTERNAL_DOTPROD OFF CACHE BOOL "Force disable dotprod" FORCE)
set(GGML_INTERNAL_SVE OFF CACHE BOOL "Force disable SVE" FORCE)

# Add third party libraries
add_subdirectory(third_party/llama.cpp)
add_subdirectory(third_party/nlohmann-json)

# Include directories
include_directories(
    ${CMAKE_CURRENT_SOURCE_DIR}/include
    ${CMAKE_CURRENT_SOURCE_DIR}/third_party/cpp-httplib
    ${CMAKE_CURRENT_SOURCE_DIR}/third_party/nlohmann-json/include
    ${CMAKE_CURRENT_SOURCE_DIR}/third_party/llama.cpp
)

# Find required packages
find_package(Threads REQUIRED)
find_package(OpenSSL REQUIRED)

# CUDA support
if(BUILD_WITH_CUDA)
    find_package(CUDAToolkit)
    if(CUDAToolkit_FOUND)
        add_definitions(-DUSE_CUDA)
        include_directories(${CUDAToolkit_INCLUDE_DIRS})
        link_libraries(${CUDAToolkit_LIBRARIES})
    else()
        message(WARNING "CUDA toolkit not found, disabling CUDA support")
        set(BUILD_WITH_CUDA OFF)
    endif()
endif()

# Metal support (macOS)
if(APPLE AND BUILD_WITH_METAL)
    find_library(METAL_FRAMEWORK Metal)
    find_library(METALKIT_FRAMEWORK MetalKit)
    find_library(FOUNDATION_FRAMEWORK Foundation)
    if(METAL_FRAMEWORK AND METALKIT_FRAMEWORK AND FOUNDATION_FRAMEWORK)
        add_definitions(-DUSE_METAL)
        link_libraries(${METAL_FRAMEWORK} ${METALKIT_FRAMEWORK} ${FOUNDATION_FRAMEWORK})
    else()
        message(WARNING "Metal frameworks not found, disabling Metal support")
        set(BUILD_WITH_METAL OFF)
    endif()
endif()

# ROCm support
if(BUILD_WITH_ROCM)
    find_package(hip)
    if(hip_FOUND)
        add_definitions(-DUSE_ROCM)
        include_directories(${HIP_INCLUDE_DIRS})
        link_libraries(${HIP_LIBRARIES})
    else()
        message(WARNING "ROCm not found, disabling ROCm support")
        set(BUILD_WITH_ROCM OFF)
    endif()
endif()

# Utils library (config etc.)
add_library(utils_lib STATIC
    ${CMAKE_CURRENT_SOURCE_DIR}/src/utils/config.cpp
)
target_include_directories(utils_lib PUBLIC ${CMAKE_CURRENT_SOURCE_DIR}/include)

# Collect source files
file(GLOB_RECURSE SOURCES
    ${CMAKE_CURRENT_SOURCE_DIR}/src/*.cpp
)

# Create executable
add_executable(ollama-node ${SOURCES})

# Link libraries
target_link_libraries(ollama-node
    llama
    utils_lib
    ${CMAKE_THREAD_LIBS_INIT}
    OpenSSL::SSL
    OpenSSL::Crypto
)

# Static linking if requested
if(STATIC_BUILD)
    if(CMAKE_CXX_COMPILER_ID MATCHES "GNU|Clang")
        set_target_properties(ollama-node PROPERTIES
            LINK_FLAGS "-static"
        )
    endif()
endif()

# Installation
install(TARGETS ollama-node
    RUNTIME DESTINATION bin
)

# Tests
if(BUILD_TESTS)
    enable_testing()
    add_subdirectory(tests)
endif()

# Print configuration summary
message(STATUS "")
message(STATUS "ollama-node-cpp configuration summary:")
message(STATUS "  Build type:        ${CMAKE_BUILD_TYPE}")
message(STATUS "  C++ standard:      ${CMAKE_CXX_STANDARD}")
message(STATUS "  Compiler:          ${CMAKE_CXX_COMPILER_ID}")
message(STATUS "  CUDA support:      ${BUILD_WITH_CUDA}")
message(STATUS "  Metal support:     ${BUILD_WITH_METAL}")
message(STATUS "  ROCm support:      ${BUILD_WITH_ROCM}")
message(STATUS "  Static build:      ${STATIC_BUILD}")
message(STATUS "  Build tests:       ${BUILD_TESTS}")
message(STATUS "")

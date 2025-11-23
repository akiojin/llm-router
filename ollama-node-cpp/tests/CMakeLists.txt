cmake_minimum_required(VERSION 3.20)

include(FetchContent)

# GoogleTestは FetchContent で取得する。リポジトリに依存物を直接持たない方針。
FetchContent_Declare(
    googletest
    URL https://github.com/google/googletest/archive/refs/tags/v1.15.2.zip
)

FetchContent_MakeAvailable(googletest)

add_library(test_common STATIC test_main.cpp)
target_link_libraries(test_common PUBLIC gtest)

target_include_directories(test_common PUBLIC
    ${CMAKE_CURRENT_SOURCE_DIR}/../include
)

set(COMMON_INCLUDE_DIRS
    ${CMAKE_CURRENT_SOURCE_DIR}/../include
)

target_include_directories(test_common PUBLIC ${COMMON_INCLUDE_DIRS})

add_subdirectory(unit)
add_subdirectory(integration)
add_subdirectory(contract)

enable_testing()

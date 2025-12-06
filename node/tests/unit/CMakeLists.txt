set(UNIT_TEST_SOURCES
    gpu_detector_test.cpp
    router_client_test.cpp
    model_sync_test.cpp
    model_downloader_test.cpp
    model_storage_test.cpp
    inference_engine_test.cpp
    model_converter_test.cpp
    hf_client_test.cpp
    llama_manager_test.cpp
    model_pool_test.cpp
    model_loader_test.cpp
    utils_config_test.cpp
    utils_misc_test.cpp
    cli_test.cpp
    ../../src/utils/cli.cpp
    ../../src/api/router_client.cpp
    ../../src/models/model_sync.cpp
    ../../src/models/model_downloader.cpp
    ../../src/models/model_storage.cpp
    ../../src/models/model_converter.cpp
    ../../src/models/hf_client.cpp
    ../../src/core/llama_manager.cpp
    ../../src/core/model_pool.cpp
    ../../src/core/inference_engine.cpp
)

# Use platform-specific gpu_detector source
if(APPLE)
    list(APPEND UNIT_TEST_SOURCES ../../src/system/gpu_detector.mm)
else()
    list(APPEND UNIT_TEST_SOURCES ../../src/system/gpu_detector.cpp)
endif()

add_executable(llm-node-unit-tests
    ${UNIT_TEST_SOURCES}
)

target_link_libraries(llm-node-unit-tests
    PRIVATE
        test_common
        llama
        common
        nlohmann_json::nlohmann_json
        utils_lib
        OpenSSL::SSL
        OpenSSL::Crypto
)

# Link Metal frameworks on macOS
if(APPLE)
    find_library(METAL_FRAMEWORK Metal)
    find_library(FOUNDATION_FRAMEWORK Foundation)
    if(METAL_FRAMEWORK AND FOUNDATION_FRAMEWORK)
        target_link_libraries(llm-node-unit-tests PRIVATE ${METAL_FRAMEWORK} ${FOUNDATION_FRAMEWORK})
        target_compile_definitions(llm-node-unit-tests PRIVATE USE_METAL=1)
    endif()
endif()

target_include_directories(llm-node-unit-tests
    PRIVATE
        ${CMAKE_CURRENT_SOURCE_DIR}/../../include
        ${CMAKE_CURRENT_SOURCE_DIR}/../../src
)

target_compile_definitions(llm-node-unit-tests
    PRIVATE
        LLM_NODE_TESTING=1
)

add_test(
    NAME llm-node-unit-tests
    COMMAND llm-node-unit-tests
)

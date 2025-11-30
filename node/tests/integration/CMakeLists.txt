set(INTEGRATION_TEST_SOURCES
    http_server_test.cpp
    openai_endpoints_test.cpp
    node_endpoints_test.cpp
    node_prometheus_test.cpp
    main_test.cpp
    model_sync_download_test.cpp
    auto_repair_test.cpp
    model_lifecycle_test.cpp
    ../../src/main.cpp
    ../../src/runtime/state.cpp
    ../../src/api/http_server.cpp
    ../../src/api/openai_endpoints.cpp
    ../../src/api/node_endpoints.cpp
    ../../src/api/router_client.cpp
    ../../src/models/model_registry.cpp
    ../../src/models/model_sync.cpp
    ../../src/models/model_downloader.cpp
    ../../src/models/ollama_compat.cpp
    ../../src/models/model_storage.cpp
    ../../src/models/model_repair.cpp
    ../../src/metrics/prometheus_exporter.cpp
    ../../src/core/inference_engine.cpp
    ../../src/core/llama_manager.cpp
)

# Use platform-specific gpu_detector source
if(APPLE)
    list(APPEND INTEGRATION_TEST_SOURCES ../../src/system/gpu_detector.mm)
else()
    list(APPEND INTEGRATION_TEST_SOURCES ../../src/system/gpu_detector.cpp)
endif()

add_executable(llm-node-integration-tests
    ${INTEGRATION_TEST_SOURCES}
)

target_link_libraries(llm-node-integration-tests
    PRIVATE
        test_common
        llama
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
        target_link_libraries(llm-node-integration-tests PRIVATE ${METAL_FRAMEWORK} ${FOUNDATION_FRAMEWORK})
        target_compile_definitions(llm-node-integration-tests PRIVATE USE_METAL=1)
    endif()
endif()

target_include_directories(llm-node-integration-tests
    PRIVATE
        ${CMAKE_CURRENT_SOURCE_DIR}/../../include
        ${CMAKE_CURRENT_SOURCE_DIR}/../../src
)

target_compile_definitions(llm-node-integration-tests
    PRIVATE
        LLM_NODE_TESTING=1
)

add_test(
    NAME llm-node-integration-tests
    COMMAND llm-node-integration-tests
)

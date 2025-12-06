set(CONTRACT_TEST_SOURCES
    openai_api_test.cpp
    router_api_test.cpp
    ../../src/api/http_server.cpp
    ../../src/api/openai_endpoints.cpp
    ../../src/api/node_endpoints.cpp
    ../../src/models/model_registry.cpp
    ../../src/models/model_sync.cpp
    ../../src/models/model_downloader.cpp
    ../../src/models/model_storage.cpp
    ../../src/metrics/prometheus_exporter.cpp
    ../../src/core/inference_engine.cpp
    ../../src/core/llama_manager.cpp
    ../../src/api/router_client.cpp
    ../../src/runtime/state.cpp
)

add_executable(llm-node-contract-tests
    ${CONTRACT_TEST_SOURCES}
)

target_link_libraries(llm-node-contract-tests
    PRIVATE
        test_common
        llama
        common
        nlohmann_json::nlohmann_json
        utils_lib
        OpenSSL::SSL
        OpenSSL::Crypto
)

target_include_directories(llm-node-contract-tests
    PRIVATE
        ${CMAKE_CURRENT_SOURCE_DIR}/../../include
        ${CMAKE_CURRENT_SOURCE_DIR}/../../src
)

add_test(
    NAME llm-node-contract-tests
    COMMAND llm-node-contract-tests
)

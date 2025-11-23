set(CONTRACT_TEST_SOURCES
    openai_api_test.cpp
    router_api_test.cpp
    ../../src/api/http_server.cpp
    ../../src/api/openai_endpoints.cpp
    ../../src/api/node_endpoints.cpp
    ../../src/models/model_registry.cpp
    ../../src/models/model_sync.cpp
    ../../src/models/model_downloader.cpp
    ../../src/core/inference_engine.cpp
    ../../src/api/router_client.cpp
)

add_executable(ollama-node-contract-tests
    ${CONTRACT_TEST_SOURCES}
)

target_link_libraries(ollama-node-contract-tests
    PRIVATE
        test_common
        nlohmann_json::nlohmann_json
        utils_lib
)

target_include_directories(ollama-node-contract-tests
    PRIVATE
        ${CMAKE_CURRENT_SOURCE_DIR}/../../include
        ${CMAKE_CURRENT_SOURCE_DIR}/../../src
)

add_test(
    NAME ollama-node-contract-tests
    COMMAND ollama-node-contract-tests
)

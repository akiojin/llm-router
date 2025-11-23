set(INTEGRATION_TEST_SOURCES
    http_server_test.cpp
    openai_endpoints_test.cpp
    node_endpoints_test.cpp
    main_test.cpp
    model_sync_download_test.cpp
    ../../src/main.cpp
    ../../src/runtime/state.cpp
    ../../src/api/http_server.cpp
    ../../src/api/openai_endpoints.cpp
    ../../src/api/node_endpoints.cpp
    ../../src/api/router_client.cpp
    ../../src/models/model_registry.cpp
    ../../src/models/model_sync.cpp
    ../../src/models/model_downloader.cpp
    ../../src/core/inference_engine.cpp
    ../../src/system/gpu_detector.cpp
)

add_executable(ollama-node-integration-tests
    ${INTEGRATION_TEST_SOURCES}
)

target_link_libraries(ollama-node-integration-tests
    PRIVATE
        test_common
        nlohmann_json::nlohmann_json
        utils_lib
)

target_include_directories(ollama-node-integration-tests
    PRIVATE
        ${CMAKE_CURRENT_SOURCE_DIR}/../../include
        ${CMAKE_CURRENT_SOURCE_DIR}/../../src
)

target_compile_definitions(ollama-node-integration-tests
    PRIVATE
        OLLAMA_NODE_TESTING=1
)

add_test(
    NAME ollama-node-integration-tests
    COMMAND ollama-node-integration-tests
)

set(UNIT_TEST_SOURCES
    gpu_detector_test.cpp
    router_client_test.cpp
    model_sync_test.cpp
    model_downloader_test.cpp
    ollama_compat_test.cpp
    inference_engine_test.cpp
    model_converter_test.cpp
    hf_client_test.cpp
    llama_manager_test.cpp
    model_pool_test.cpp
    utils_config_test.cpp
    ../../src/system/gpu_detector.cpp
    ../../src/api/router_client.cpp
    ../../src/models/model_sync.cpp
    ../../src/models/model_downloader.cpp
    ../../src/models/ollama_compat.cpp
    ../../src/models/model_converter.cpp
    ../../src/models/hf_client.cpp
    ../../src/core/llama_manager.cpp
    ../../src/core/model_pool.cpp
    ../../src/core/inference_engine.cpp
)

add_executable(ollama-node-unit-tests
    ${UNIT_TEST_SOURCES}
)

target_link_libraries(ollama-node-unit-tests
    PRIVATE
        test_common
        nlohmann_json::nlohmann_json
        utils_lib
        OpenSSL::SSL
        OpenSSL::Crypto
)

target_include_directories(ollama-node-unit-tests
    PRIVATE
        ${CMAKE_CURRENT_SOURCE_DIR}/../../include
        ${CMAKE_CURRENT_SOURCE_DIR}/../../src
)

target_compile_definitions(ollama-node-unit-tests
    PRIVATE
        OLLAMA_NODE_TESTING=1
)

add_test(
    NAME ollama-node-unit-tests
    COMMAND ollama-node-unit-tests
)

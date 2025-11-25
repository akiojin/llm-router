#include <iostream>
#include <memory>
#include <signal.h>
#include <atomic>
#include <thread>
#include <chrono>
#include <string>
#include <vector>
#include <unistd.h>

#include "system/gpu_detector.h"
#include "api/router_client.h"
#include "models/model_sync.h"
#include "models/model_registry.h"
#include "core/inference_engine.h"
#include "api/openai_endpoints.h"
#include "api/node_endpoints.h"
#include "api/http_server.h"
#include "utils/config.h"
#include "runtime/state.h"
#include "utils/logger.h"

int run_node(const ollama_node::NodeConfig& cfg, bool single_iteration) {
    ollama_node::g_running_flag.store(true);

    bool server_started = false;
    std::thread heartbeat_thread;

    try {
        ollama_node::logger::init_from_env();
        ollama_node::set_ready(false);
        std::string router_url = cfg.router_url;
        int node_port = cfg.node_port;

        spdlog::info("Router URL: {}", router_url);
        spdlog::info("Node port: {}", node_port);

        // GPU detection
        std::cout << "Detecting GPUs..." << std::endl;
        ollama_node::GpuDetector gpu_detector;
        auto gpus = gpu_detector.detect();
        if (cfg.require_gpu && !gpu_detector.hasGpu()) {
            std::cerr << "Error: No GPU detected. GPU is required for node operation." << std::endl;
            return 1;
        }
        size_t total_mem = gpu_detector.getTotalMemory();
        double capability = gpu_detector.getCapabilityScore();
        std::cout << "GPU detected: devices=" << gpus.size() << " total_mem=" << total_mem << " bytes" << std::endl;

        // Build GPU device info for router
        std::vector<ollama_node::GpuDeviceInfoForRouter> gpu_devices;
        for (const auto& gpu : gpus) {
            if (gpu.is_available) {
                ollama_node::GpuDeviceInfoForRouter device;
                device.model = gpu.name;
                device.count = 1;
                device.memory = gpu.memory_bytes;
                gpu_devices.push_back(device);
            }
        }

        // Get machine name from hostname
        char hostname_buf[256] = "localhost";
        gethostname(hostname_buf, sizeof(hostname_buf));

        std::string bind_address = cfg.bind_address.empty() ? std::string("0.0.0.0") : cfg.bind_address;

        // Initialize model registry (empty for now, will sync after registration)
        ollama_node::ModelRegistry registry;

        // Initialize inference engine
        ollama_node::InferenceEngine engine;

        // Start HTTP server BEFORE registration (router checks /v1/models endpoint)
        ollama_node::OpenAIEndpoints openai(registry, engine);
        ollama_node::NodeEndpoints node_endpoints;
        node_endpoints.setGpuInfo(gpus.size(), total_mem, capability);
        ollama_node::HttpServer server(node_port, openai, node_endpoints, bind_address);
        std::cout << "Starting HTTP server on port " << node_port << "..." << std::endl;
        server.start();
        server_started = true;

        // Register with router (retry)
        std::cout << "Registering with router..." << std::endl;
        ollama_node::RouterClient router(router_url);
        ollama_node::NodeInfo info;
        info.machine_name = hostname_buf;
        info.ip_address = "127.0.0.1";  // TODO: detect actual IP
        info.ollama_version = "1.0.0";  // ollama-node-cpp version
        // Router calculates API port as ollama_port + 1, so report node_port - 1
        info.ollama_port = static_cast<uint16_t>(node_port > 0 ? node_port - 1 : 11434);
        info.gpu_available = !gpu_devices.empty();
        info.gpu_devices = gpu_devices;
        if (!gpu_devices.empty()) {
            info.gpu_count = static_cast<uint32_t>(gpu_devices.size());
            info.gpu_model = gpu_devices[0].model;
        }
        ollama_node::NodeRegistrationResult reg;
        const int reg_max = 3;
        for (int attempt = 0; attempt < reg_max; ++attempt) {
            reg = router.registerNode(info);
            if (reg.success) break;
            std::this_thread::sleep_for(std::chrono::milliseconds(200 * (attempt + 1)));
        }
        if (!reg.success) {
            std::cerr << "Router registration failed after retries: " << reg.error << std::endl;
            server.stop();
            return 1;
        }

        // Sync models from router
        std::cout << "Syncing models from router..." << std::endl;
        std::string models_dir = cfg.models_dir.empty()
                                     ? std::string(getenv("HOME") ? getenv("HOME") : ".") + "/.ollama/models"
                                     : cfg.models_dir;
        ollama_node::ModelSync model_sync(router_url, models_dir);
        auto sync_result = model_sync.sync();
        if (sync_result.to_download.empty() && sync_result.to_delete.empty() && model_sync.listLocalModels().empty()) {
            // If nothing synced and no local models, treat as recoverable error and retry once
            std::this_thread::sleep_for(std::chrono::milliseconds(200));
            sync_result = model_sync.sync();
        }
        // Update registry with synced models
        registry.setModels(model_sync.fetchRemoteModels());

        ollama_node::set_ready(true);

        // Heartbeat thread
        std::cout << "Starting heartbeat thread..." << std::endl;
        heartbeat_thread = std::thread([&]() {
            while (ollama_node::is_running()) {
                if (reg.success) {
                    router.sendHeartbeat(reg.node_id);
                }
                std::this_thread::sleep_for(std::chrono::seconds(cfg.heartbeat_interval_sec));
            }
        });

        std::cout << "Node initialized successfully, ready to serve requests" << std::endl;

        // Main loop
        if (single_iteration) {
            std::this_thread::sleep_for(std::chrono::milliseconds(500));
            ollama_node::request_shutdown();
        }
        while (ollama_node::is_running()) {
            std::this_thread::sleep_for(std::chrono::seconds(1));
        }

        // Cleanup
        std::cout << "Shutting down..." << std::endl;
        server.stop();
        if (heartbeat_thread.joinable()) {
            heartbeat_thread.join();
        }

    } catch (const std::exception& e) {
        std::cerr << "Fatal error: " << e.what() << std::endl;
        if (heartbeat_thread.joinable()) {
            ollama_node::request_shutdown();
            heartbeat_thread.join();
        }
        if (server_started) {
            // best-effort stop
        }
        return 1;
    }

    std::cout << "Node shutdown complete" << std::endl;
    return 0;
}

void signalHandler(int signal) {
    std::cout << "Received signal " << signal << ", shutting down..." << std::endl;
    ollama_node::request_shutdown();
}

#ifndef OLLAMA_NODE_TESTING
int main(int argc, char* argv[]) {
    // Set up signal handlers
    signal(SIGINT, signalHandler);
    signal(SIGTERM, signalHandler);

    std::cout << "ollama-node-cpp v1.0.0 starting..." << std::endl;

    auto cfg = ollama_node::loadNodeConfig();
    return run_node(cfg, /*single_iteration=*/false);
}
#endif

#ifdef OLLAMA_NODE_TESTING
extern "C" int ollama_node_run_for_test() {
    auto cfg = ollama_node::loadNodeConfig();
    // short intervals for tests
    cfg.heartbeat_interval_sec = 1;
    cfg.require_gpu = false;
    return run_node(cfg, /*single_iteration=*/true);
}
#endif

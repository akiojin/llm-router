#include "system/gpu_detector.h"
#include <iostream>
#include <sstream>
#include <cstring>

#ifdef USE_CUDA
#include <cuda_runtime.h>
#include <nvml.h>
#endif

#ifdef USE_ROCM
#include <rocm_smi/rocm_smi.h>
#endif

// Metal implementation is in gpu_detector.mm for macOS
#ifndef __APPLE__

namespace llm_node {

GpuDetector::GpuDetector() {
    // Constructor
}

GpuDetector::~GpuDetector() {
    // Destructor
}

std::vector<GpuDevice> GpuDetector::detect() {
    detected_devices_.clear();

#ifdef USE_CUDA
    auto cuda_devices = detectCuda();
    detected_devices_.insert(detected_devices_.end(), cuda_devices.begin(), cuda_devices.end());
#endif

#ifdef USE_ROCM
    auto rocm_devices = detectRocm();
    detected_devices_.insert(detected_devices_.end(), rocm_devices.begin(), rocm_devices.end());
#endif

    // If no GPU support is compiled in, try to detect basic GPU presence
    if (detected_devices_.empty()) {
        // Try basic detection via system calls or file system
        // This is a fallback for development/testing
#ifdef __linux__
        // Check for NVIDIA GPUs via /proc/driver/nvidia/gpus/
        if (std::system("which nvidia-smi > /dev/null 2>&1") == 0) {
            // NVIDIA GPU likely present but CUDA not enabled
            GpuDevice dev;
            dev.id = 0;
            dev.name = "NVIDIA GPU (CUDA support not compiled)";
            dev.memory_bytes = 0;  // Unknown
            dev.compute_capability = "unknown";
            dev.vendor = "nvidia";
            dev.is_available = false;  // Not usable without CUDA
            detected_devices_.push_back(dev);
        }
#endif
    }

    return detected_devices_;
}

bool GpuDetector::hasGpu() const {
    // Check if any GPU is available and usable
    for (const auto& dev : detected_devices_) {
        if (dev.is_available) {
            return true;
        }
    }
    return false;
}

bool GpuDetector::requireGpu() const {
    return hasGpu();
}

std::unique_ptr<GpuDevice> GpuDetector::getGpuById(int id) const {
    for (const auto& dev : detected_devices_) {
        if (dev.id == id) {
            return std::make_unique<GpuDevice>(dev);
        }
    }
    return nullptr;
}

size_t GpuDetector::getTotalMemory() const {
    size_t total = 0;
    for (const auto& dev : detected_devices_) {
        if (dev.is_available) {
            total += dev.memory_bytes;
        }
    }
    return total;
}

double GpuDetector::getCapabilityScore() const {
    // Calculate a capability score based on memory and compute capability
    // This is used by the router for load balancing
    double score = 0.0;

    for (const auto& dev : detected_devices_) {
        if (!dev.is_available) continue;

        // Base score from memory (GB)
        double mem_score = static_cast<double>(dev.memory_bytes) / (1024.0 * 1024.0 * 1024.0);

        // Multiply by compute capability factor
        double cc_factor = 1.0;
        if (dev.vendor == "nvidia") {
            // Parse compute capability (e.g., "8.6" -> 8.6)
            try {
                double cc = std::stod(dev.compute_capability);
                cc_factor = cc / 5.0;  // Normalize around 5.0 as baseline
            } catch (...) {
                cc_factor = 1.0;
            }
        } else if (dev.vendor == "amd") {
            cc_factor = 1.2;  // AMD GPUs
        } else if (dev.vendor == "apple") {
            cc_factor = 1.5;  // Apple Silicon
        }

        score += mem_score * cc_factor;
    }

    return score;
}

std::vector<GpuDevice> GpuDetector::detectCuda() {
    std::vector<GpuDevice> devices;

#ifdef USE_CUDA
    int device_count = 0;
    cudaError_t cuda_err = cudaGetDeviceCount(&device_count);

    if (cuda_err != cudaSuccess || device_count == 0) {
        return devices;
    }

    // Initialize NVML for additional GPU info
    nvmlReturn_t nvml_result = nvmlInit();
    bool nvml_available = (nvml_result == NVML_SUCCESS);

    for (int i = 0; i < device_count; ++i) {
        cudaDeviceProp prop;
        cuda_err = cudaGetDeviceProperties(&prop, i);

        if (cuda_err != cudaSuccess) continue;

        GpuDevice dev;
        dev.id = i;
        dev.name = prop.name;
        dev.memory_bytes = prop.totalGlobalMem;

        // Format compute capability
        std::stringstream ss;
        ss << prop.major << "." << prop.minor;
        dev.compute_capability = ss.str();

        dev.vendor = "nvidia";
        dev.is_available = true;

        // Try to get additional info from NVML if available
        if (nvml_available) {
            nvmlDevice_t nvml_device;
            if (nvmlDeviceGetHandleByIndex(i, &nvml_device) == NVML_SUCCESS) {
                // Could get temperature, utilization, etc. here if needed
            }
        }

        devices.push_back(dev);
    }

    if (nvml_available) {
        nvmlShutdown();
    }
#endif

    return devices;
}

std::vector<GpuDevice> GpuDetector::detectMetal() {
    // Metal is only available on macOS, handled in gpu_detector.mm
    return std::vector<GpuDevice>();
}

std::vector<GpuDevice> GpuDetector::detectRocm() {
    std::vector<GpuDevice> devices;

#ifdef USE_ROCM
    // ROCm detection for AMD GPUs
    rsmi_status_t ret = rsmi_init(0);
    if (ret != RSMI_STATUS_SUCCESS) {
        return devices;
    }

    uint32_t device_count = 0;
    ret = rsmi_num_monitor_devices(&device_count);

    if (ret == RSMI_STATUS_SUCCESS && device_count > 0) {
        for (uint32_t i = 0; i < device_count; ++i) {
            GpuDevice dev;
            dev.id = static_cast<int>(i);

            // Get device name
            char name[256];
            ret = rsmi_dev_name_get(i, name, sizeof(name));
            if (ret == RSMI_STATUS_SUCCESS) {
                dev.name = name;
            } else {
                dev.name = "AMD GPU";
            }

            // Get memory info
            uint64_t total_mem = 0;
            ret = rsmi_dev_memory_total_get(i, RSMI_MEM_TYPE_VRAM, &total_mem);
            if (ret == RSMI_STATUS_SUCCESS) {
                dev.memory_bytes = total_mem;
            }

            // ROCm doesn't have a direct compute capability equivalent
            dev.compute_capability = "gfx";  // Could be more specific with device ID
            dev.vendor = "amd";
            dev.is_available = true;

            devices.push_back(dev);
        }
    }

    rsmi_shut_down();
#endif

    return devices;
}

#ifdef LLM_NODE_TESTING
void GpuDetector::setDetectedDevicesForTest(std::vector<GpuDevice> devices) {
    detected_devices_ = std::move(devices);
}
#endif

} // namespace llm_node

#endif // !__APPLE__

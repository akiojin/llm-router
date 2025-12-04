#include <gtest/gtest.h>

#include "system/gpu_detector.h"

namespace {

using llm_node::GpuDetector;

TEST(GpuDetectorSmokeTest, DefaultsAreEmpty) {
    GpuDetector detector;

    EXPECT_FALSE(detector.hasGpu());
    EXPECT_EQ(detector.getTotalMemory(), 0u);
    EXPECT_DOUBLE_EQ(detector.getCapabilityScore(), 0.0);
    EXPECT_EQ(detector.getGpuById(0), nullptr);
}

TEST(GpuDetectorTest, TotalMemorySumsAvailableDevicesOnly) {
    GpuDetector detector;

    std::vector<llm_node::GpuDevice> devices = {
        {0, "NVIDIA A100", 40ull * 1024 * 1024 * 1024, "8.0", "nvidia", true},
        {1, "AMD Test", 16ull * 1024 * 1024 * 1024, "gfx1100", "amd", false},
        {2, "Apple M3", 8ull * 1024 * 1024 * 1024, "Metal3", "apple", true},
    };

    detector.setDetectedDevicesForTest(devices);

    // Unavailable AMD GPUはメモリ計算から除外される想定
    const size_t expected = (40ull + 8ull) * 1024 * 1024 * 1024;
    EXPECT_EQ(detector.getTotalMemory(), expected);
}

TEST(GpuDetectorTest, CapabilityScoreWeightsByVendorAndComputeCapability) {
    GpuDetector detector;

    std::vector<llm_node::GpuDevice> devices = {
        {0, "NVIDIA 8GB", 8ull * 1024 * 1024 * 1024, "8.6", "nvidia", true},
        {1, "AMD 16GB", 16ull * 1024 * 1024 * 1024, "gfx1100", "amd", true},
        {2, "Apple 4GB", 4ull * 1024 * 1024 * 1024, "Metal3", "apple", true},
    };

    detector.setDetectedDevicesForTest(devices);

    const double nvidia = 8.0 * (8.6 / 5.0);
    const double amd = 16.0 * 1.2;
    const double apple = 4.0 * 1.5;
    const double expected = nvidia + amd + apple;

    EXPECT_NEAR(detector.getCapabilityScore(), expected, 1e-6);
}

TEST(GpuDetectorTest, RequireGpuReflectsAvailability) {
    GpuDetector detector;
    detector.setDetectedDevicesForTest({});
    EXPECT_FALSE(detector.requireGpu());

    std::vector<llm_node::GpuDevice> devices = {
        {0, "NVIDIA", 8ull * 1024 * 1024 * 1024, "8.0", "nvidia", true},
        {1, "Disabled", 4ull * 1024 * 1024 * 1024, "5.0", "nvidia", false},
    };
    detector.setDetectedDevicesForTest(devices);
    EXPECT_TRUE(detector.requireGpu());
}

}  // namespace

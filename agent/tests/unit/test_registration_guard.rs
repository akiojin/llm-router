use ollama_coordinator_agent::registration::gpu_devices_valid;
use ollama_coordinator_common::types::GpuDeviceInfo;

#[test]
fn gpu_devices_validation_requires_entries() {
    let devices: Vec<GpuDeviceInfo> = Vec::new();
    assert!(
        !gpu_devices_valid(&devices),
        "GPU情報が空でも登録可能と判定されてはならない"
    );
}

#[test]
fn gpu_devices_validation_accepts_positive_counts() {
    let devices = vec![GpuDeviceInfo {
        model: "NVIDIA RTX 4090".to_string(),
        count: 2,
        memory: None,
    }];
    assert!(gpu_devices_valid(&devices));
}

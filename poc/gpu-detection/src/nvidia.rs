use std::fs;
use std::path::Path;

pub struct NvidiaDetectionResult {
    pub detected: bool,
    pub method: Option<String>,
    pub details: Vec<String>,
}

pub fn detect_nvidia() -> NvidiaDetectionResult {
    let mut result = NvidiaDetectionResult {
        detected: false,
        method: None,
        details: Vec::new(),
    };

    // Method 1: Check device files
    result.details.push("=== Method 1: Device Files ===".to_string());
    if Path::new("/dev/nvidia0").exists() {
        result.detected = true;
        result.method = Some("device_file".to_string());
        result.details.push("✓ /dev/nvidia0 exists".to_string());
    } else {
        result.details.push("✗ /dev/nvidia0 not found".to_string());
    }

    // Check other nvidia device files
    for i in 0..8 {
        let device_path = format!("/dev/nvidia{}", i);
        if Path::new(&device_path).exists() {
            result.details.push(format!("✓ {} exists", device_path));
        }
    }

    if Path::new("/dev/nvidiactl").exists() {
        result.details.push("✓ /dev/nvidiactl exists".to_string());
    }

    if Path::new("/dev/nvidia-modeset").exists() {
        result.details.push("✓ /dev/nvidia-modeset exists".to_string());
    }

    // Method 2: Check /proc/driver/nvidia/version
    result.details.push("\n=== Method 2: Driver Version File ===".to_string());
    if let Ok(version) = fs::read_to_string("/proc/driver/nvidia/version") {
        result.detected = true;
        if result.method.is_none() {
            result.method = Some("proc_driver".to_string());
        }
        result.details.push(format!("✓ NVIDIA driver version:\n{}", version.trim()));
    } else {
        result.details.push("✗ /proc/driver/nvidia/version not found".to_string());
    }

    // Method 3: Check library paths (like Ollama does)
    result.details.push("\n=== Method 3: Library Search ===".to_string());
    let library_patterns = vec![
        "/usr/local/cuda/lib64/libnvidia-ml.so*",
        "/usr/lib/x86_64-linux-gnu/nvidia/current/libnvidia-ml.so*",
        "/usr/lib/x86_64-linux-gnu/libnvidia-ml.so*",
        "/usr/lib/wsl/lib/libnvidia-ml.so*",
        "/opt/cuda/lib64/libnvidia-ml.so*",
        "/usr/lib64/libnvidia-ml.so*",
    ];

    for pattern in library_patterns {
        match glob::glob(pattern) {
            Ok(paths) => {
                let found_paths: Vec<_> = paths.filter_map(Result::ok).collect();
                if !found_paths.is_empty() {
                    result.detected = true;
                    if result.method.is_none() {
                        result.method = Some("library_search".to_string());
                    }
                    for path in found_paths {
                        result.details.push(format!("✓ Found: {}", path.display()));
                    }
                } else {
                    result.details.push(format!("✗ No match: {}", pattern));
                }
            }
            Err(e) => {
                result.details.push(format!("✗ Glob error for {}: {}", pattern, e));
            }
        }
    }

    result
}

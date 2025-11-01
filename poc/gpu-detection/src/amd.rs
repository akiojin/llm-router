use std::fs;
use std::path::Path;

pub struct AmdDetectionResult {
    pub detected: bool,
    pub method: Option<String>,
    pub details: Vec<String>,
}

pub fn detect_amd() -> AmdDetectionResult {
    let mut result = AmdDetectionResult {
        detected: false,
        method: None,
        details: Vec::new(),
    };

    // Method 1: Check /dev/kfd (Kernel Fusion Driver)
    result.details.push("=== Method 1: KFD Device File ===".to_string());
    if Path::new("/dev/kfd").exists() {
        result.detected = true;
        result.method = Some("kfd_device".to_string());
        result.details.push("✓ /dev/kfd exists".to_string());
    } else {
        result.details.push("✗ /dev/kfd not found".to_string());
    }

    // Method 2: Check sysfs KFD topology (like Ollama v0.1.29+)
    result.details.push("\n=== Method 2: KFD Topology (sysfs) ===".to_string());
    let kfd_path = "/sys/class/kfd/kfd/topology/nodes";

    if Path::new(kfd_path).exists() {
        result.details.push(format!("✓ {} exists", kfd_path));

        // Read node directories
        if let Ok(entries) = fs::read_dir(kfd_path) {
            for entry in entries.flatten() {
                let properties_path = entry.path().join("properties");
                if properties_path.exists() {
                    result.details.push(format!("\n  Checking: {}", properties_path.display()));

                    if let Ok(content) = fs::read_to_string(&properties_path) {
                        // Look for vendor_id 0x1002 (AMD)
                        for line in content.lines() {
                            if line.contains("vendor_id") {
                                result.details.push(format!("    {}", line.trim()));
                                if line.contains("0x1002") {
                                    result.detected = true;
                                    if result.method.is_none() {
                                        result.method = Some("kfd_sysfs".to_string());
                                    }
                                    result.details.push("    ✓ AMD vendor_id detected!".to_string());
                                }
                            }
                        }
                    }
                }
            }
        }
    } else {
        result.details.push(format!("✗ {} not found", kfd_path));
    }

    // Method 3: Check DRM devices
    result.details.push("\n=== Method 3: DRM Devices ===".to_string());
    let drm_path = "/sys/class/drm";

    if Path::new(drm_path).exists() {
        if let Ok(entries) = fs::read_dir(drm_path) {
            for entry in entries.flatten() {
                let card_name = entry.file_name();
                let card_name_str = card_name.to_string_lossy();

                // Check card* and renderD* devices
                if card_name_str.starts_with("card") || card_name_str.starts_with("renderD") {
                    let vendor_path = entry.path().join("device/vendor");
                    if vendor_path.exists() {
                        if let Ok(vendor) = fs::read_to_string(&vendor_path) {
                            let vendor = vendor.trim();
                            result.details.push(format!("  {}: vendor = {}", card_name_str, vendor));

                            if vendor == "0x1002" {
                                result.detected = true;
                                if result.method.is_none() {
                                    result.method = Some("drm_device".to_string());
                                }
                                result.details.push(format!("    ✓ AMD GPU detected in {}", card_name_str));
                            }
                        }
                    }
                }
            }
        }
    } else {
        result.details.push(format!("✗ {} not found", drm_path));
    }

    // Method 4: Check /dev/dri/renderD* devices
    result.details.push("\n=== Method 4: DRI Render Devices ===".to_string());
    for i in 128..136 {
        let render_path = format!("/dev/dri/renderD{}", i);
        if Path::new(&render_path).exists() {
            result.details.push(format!("✓ {} exists", render_path));
        }
    }

    result
}

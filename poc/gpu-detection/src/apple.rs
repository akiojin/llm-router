use std::fs;
use std::process::Command;

pub struct AppleDetectionResult {
    pub detected: bool,
    pub method: Option<String>,
    pub details: Vec<String>,
}

pub fn detect_apple() -> AppleDetectionResult {
    let mut result = AppleDetectionResult {
        detected: false,
        method: None,
        details: Vec::new(),
    };

    // Method 1: Check lscpu output for "Vendor ID: Apple"
    result.details.push("=== Method 1: lscpu ===".to_string());
    if let Ok(output) = Command::new("lscpu").output() {
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            result.details.push("✓ lscpu command succeeded".to_string());

            for line in stdout.lines() {
                if line.contains("Vendor ID") {
                    result.details.push(format!("  {}", line.trim()));
                    if line.contains("Apple") {
                        result.detected = true;
                        result.method = Some("lscpu".to_string());
                        result.details.push("  ✓ Apple Silicon detected!".to_string());
                    }
                }
            }

            // Also check for ARM architecture
            for line in stdout.lines() {
                if line.contains("Architecture:") {
                    result.details.push(format!("  {}", line.trim()));
                }
            }
        } else {
            result.details.push("✗ lscpu command failed".to_string());
        }
    } else {
        result.details.push("✗ lscpu command not available".to_string());
    }

    // Method 2: Check /proc/cpuinfo for CPU implementer 0x61 (Apple)
    result.details.push("\n=== Method 2: /proc/cpuinfo ===".to_string());
    if let Ok(content) = fs::read_to_string("/proc/cpuinfo") {
        result.details.push("✓ /proc/cpuinfo exists".to_string());

        let mut found_implementer = false;
        for line in content.lines() {
            if line.contains("CPU implementer") {
                found_implementer = true;
                result.details.push(format!("  {}", line.trim()));

                // Apple Silicon uses implementer 0x61
                if line.contains("0x61") {
                    result.detected = true;
                    if result.method.is_none() {
                        result.method = Some("cpuinfo".to_string());
                    }
                    result.details.push("  ✓ Apple implementer (0x61) detected!".to_string());
                }
            }

            // Also show CPU part and variant
            if line.contains("CPU architecture") || line.contains("CPU variant") || line.contains("CPU part") {
                result.details.push(format!("  {}", line.trim()));
            }
        }

        if !found_implementer {
            result.details.push("  ✗ No CPU implementer field found".to_string());
        }
    } else {
        result.details.push("✗ /proc/cpuinfo not found".to_string());
    }

    // Method 3: Check for macOS-specific sysctl (only works on native macOS)
    #[cfg(target_os = "macos")]
    {
        result.details.push("\n=== Method 3: sysctl (macOS native) ===".to_string());
        if let Ok(output) = Command::new("sysctl")
            .args(&["-n", "machdep.cpu.brand_string"])
            .output()
        {
            if output.status.success() {
                let brand = String::from_utf8_lossy(&output.stdout);
                result.details.push(format!("  CPU Brand: {}", brand.trim()));

                if brand.contains("Apple") {
                    result.detected = true;
                    if result.method.is_none() {
                        result.method = Some("sysctl".to_string());
                    }
                    result.details.push("  ✓ Apple CPU detected via sysctl!".to_string());
                }
            }
        }

        // Check for hw.perflevelX values (Apple Silicon specific)
        if let Ok(output) = Command::new("sysctl")
            .args(&["-n", "hw.perflevel0.physicalcpu"])
            .output()
        {
            if output.status.success() {
                let cores = String::from_utf8_lossy(&output.stdout);
                result.details.push(format!("  Performance cores: {}", cores.trim()));
            }
        }
    }

    #[cfg(not(target_os = "macos"))]
    {
        result.details.push("\n=== Method 3: sysctl (macOS native) ===".to_string());
        result.details.push("  ⊘ Not running on macOS - sysctl checks skipped".to_string());
    }

    result
}

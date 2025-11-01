mod nvidia;
mod amd;
mod apple;

use nvidia::detect_nvidia;
use amd::detect_amd;
use apple::detect_apple;

fn main() {
    println!("=======================================================");
    println!("       GPU Detection PoC for Ollama Coordinator");
    println!("=======================================================\n");

    // Detect NVIDIA GPUs
    println!("\n### NVIDIA GPU Detection ###\n");
    let nvidia_result = detect_nvidia();
    for detail in &nvidia_result.details {
        println!("{}", detail);
    }
    println!("\n[NVIDIA] Detected: {}", nvidia_result.detected);
    if let Some(method) = &nvidia_result.method {
        println!("[NVIDIA] Detection method: {}", method);
    }

    // Detect AMD GPUs
    println!("\n\n### AMD GPU Detection ###\n");
    let amd_result = detect_amd();
    for detail in &amd_result.details {
        println!("{}", detail);
    }
    println!("\n[AMD] Detected: {}", amd_result.detected);
    if let Some(method) = &amd_result.method {
        println!("[AMD] Detection method: {}", method);
    }

    // Detect Apple Silicon
    println!("\n\n### Apple Silicon Detection ###\n");
    let apple_result = detect_apple();
    for detail in &apple_result.details {
        println!("{}", detail);
    }
    println!("\n[Apple Silicon] Detected: {}", apple_result.detected);
    if let Some(method) = &apple_result.method {
        println!("[Apple Silicon] Detection method: {}", method);
    }

    // Summary
    println!("\n\n=======================================================");
    println!("                      SUMMARY");
    println!("=======================================================");

    let mut detected_gpus = Vec::new();
    if nvidia_result.detected {
        detected_gpus.push(format!("NVIDIA (via {})", nvidia_result.method.unwrap_or_default()));
    }
    if amd_result.detected {
        detected_gpus.push(format!("AMD (via {})", amd_result.method.unwrap_or_default()));
    }
    if apple_result.detected {
        detected_gpus.push(format!("Apple Silicon (via {})", apple_result.method.unwrap_or_default()));
    }

    if detected_gpus.is_empty() {
        println!("No GPU detected by any method.");
    } else {
        println!("Detected GPUs:");
        for gpu in detected_gpus {
            println!("  - {}", gpu);
        }
    }

    println!("=======================================================\n");
}

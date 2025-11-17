//! Coordinator GPU-focused integration tests entrypoint

#[path = "support/mod.rs"]
pub mod support;

#[path = "integration/registry_cleanup.rs"]
mod registry_cleanup;

#[path = "integration/dashboard_gpu_display.rs"]
mod dashboard_gpu_display;

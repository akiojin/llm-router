//! Unit tests entrypoint for model management

#[path = "unit/gpu_model_selector_test.rs"]
mod gpu_model_selector_test;

#[path = "unit/model_repository_test.rs"]
mod model_repository_test;

// Tests are defined inside the modules; this harness ensures they are built
// and executed when running `cargo test`.

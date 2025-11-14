//! Integration tests entrypoint for model management

#[path = "integration/auto_download_test.rs"]
mod auto_download_test;

#[path = "integration/manual_distribution_test.rs"]
mod manual_distribution_test;

#[path = "integration/model_info_test.rs"]
mod model_info_test;

// Tests are defined inside the modules; this harness ensures they are built
// and executed when running `cargo test`.

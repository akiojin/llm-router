//! Coordinator load balancing integration tests entrypoint

#[path = "integration/test_load_balancing.rs"]
mod test_load_balancing;

#[path = "integration/test_metrics.rs"]
mod test_metrics;

#[path = "integration/loadbalancer_test.rs"]
mod loadbalancer_test;

// Tests are defined inside the module; this harness ensures they are built
// and executed when running `cargo test`.

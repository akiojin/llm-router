//! Coordinator contract tests entrypoint

#[path = "support/mod.rs"]
pub mod support;

#[path = "contract/test_health_check.rs"]
mod test_health_check;

#[path = "contract/test_agents_list.rs"]
mod test_agents_list;

#[path = "contract/test_agent_registration.rs"]
mod test_agent_registration;

#[path = "contract/test_agent_register_gpu.rs"]
mod test_agent_register_gpu;

#[path = "contract/test_proxy_chat.rs"]
mod test_proxy_chat;

#[path = "contract/test_proxy_completions.rs"]
mod test_proxy_completions;
#[path = "contract/test_proxy_generate.rs"]
mod test_proxy_generate;

#[path = "contract/test_metrics.rs"]
mod test_metrics;

#[path = "contract/models_api_test.rs"]
mod models_api_test;

// Tests are defined inside the modules; this harness ensures they are built
// and executed when running `cargo test`.

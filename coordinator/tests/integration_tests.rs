//! Integration tests entrypoint for model management
//! （現行仕様では手動配布/旧自動配布テストを削除済み）

#[path = "support/mod.rs"]
mod support;

#[path = "integration/model_info_test.rs"]
mod model_info_test;

#[path = "contract/test_proxy_completions.rs"]
mod test_proxy_completions;

// Tests are defined inside the modules; this harness ensures they are built
// and executed when running `cargo test`.

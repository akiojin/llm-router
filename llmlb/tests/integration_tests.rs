//! Integration tests entrypoint for model management
//! （現行仕様では手動配布/旧自動配布テストを削除済み）

#[path = "support/mod.rs"]
mod support;

#[path = "integration/model_info_test.rs"]
mod model_info_test;

#[path = "integration/models_metadata_test.rs"]
mod models_metadata_test;

#[path = "integration/migration_test.rs"]
mod migration_test;

// NOTE: audio_api_test.rs was deleted as part of NodeRegistry removal (SPEC-e8e9326e)
// All tests depended on deprecated /api/internal/test/register-node endpoint

#[path = "integration/images_api_test.rs"]
mod images_api_test;

#[path = "integration/api_key_scopes_test.rs"]
mod api_key_scopes_test;

#[path = "contract/test_proxy_completions.rs"]
mod test_proxy_completions;

#[path = "integration/test_dashboard.rs"]
mod test_dashboard;

// SPEC-e8e9326e: エンドポイント管理統合テスト
#[path = "integration/endpoint_registration_test.rs"]
mod endpoint_registration_test;

#[path = "integration/endpoint_health_check_test.rs"]
mod endpoint_health_check_test;

#[path = "integration/endpoint_model_sync_test.rs"]
mod endpoint_model_sync_test;

#[path = "integration/endpoint_connection_test_test.rs"]
mod endpoint_connection_test_test;

#[path = "integration/endpoint_management_test.rs"]
mod endpoint_management_test;

#[path = "integration/endpoint_name_uniqueness_test.rs"]
mod endpoint_name_uniqueness_test;

#[path = "integration/endpoint_latency_routing_test.rs"]
mod endpoint_latency_routing_test;

#[path = "integration/endpoint_auto_recovery_test.rs"]
mod endpoint_auto_recovery_test;

#[path = "integration/endpoint_viewer_access_test.rs"]
mod endpoint_viewer_access_test;

// SPEC-e8e9326e: エンドポイントタイプ関連の統合テスト
#[path = "integration/endpoint_type_detection_test.rs"]
mod endpoint_type_detection_test;

#[path = "integration/endpoint_type_filter_test.rs"]
mod endpoint_type_filter_test;

#[path = "integration/endpoint_type_auto_detection_test.rs"]
mod endpoint_type_auto_detection_test;

#[path = "integration/dashboard_endpoints_type_test.rs"]
mod dashboard_endpoints_type_test;

#[path = "integration/endpoint_xllm_download_test.rs"]
mod endpoint_xllm_download_test;

#[path = "integration/endpoint_download_reject_test.rs"]
mod endpoint_download_reject_test;

#[path = "integration/endpoint_model_metadata_test.rs"]
mod endpoint_model_metadata_test;

// SPEC-fbc50d97: リクエスト履歴ストレージ/キャプチャ統合テスト
#[path = "integration/request_storage_test.rs"]
mod request_storage_test;

#[path = "integration/request_capture_test.rs"]
mod request_capture_test;

// 既存の統合テスト（ハーネス未登録だったものを追加）
#[path = "integration/test_metrics.rs"]
mod test_metrics;

#[path = "integration/test_node_lifecycle.rs"]
mod test_node_lifecycle;

#[path = "integration/test_health_monitor.rs"]
mod test_health_monitor;

#[path = "integration/test_proxy.rs"]
mod test_proxy;

// SPEC-0f1de549: Open Responses API統合テスト
#[path = "integration/responses_api_test.rs"]
mod responses_api_test;

#[path = "integration/responses_streaming_test.rs"]
mod responses_streaming_test;

#[path = "integration/models_api_test.rs"]
mod models_api_test;

#[path = "integration/model_routing_balancing_test.rs"]
mod model_routing_balancing_test;
// SPEC-a6e55b37: self-update drain gate
#[path = "integration/update_drain_gate_test.rs"]
mod update_drain_gate_test;

// SPEC-a6e55b37: self-update apply API
#[path = "integration/system_update_apply_api_test.rs"]
mod system_update_apply_api_test;

// SPEC-f8e3a1b7: /api/system API統合テスト
#[path = "integration/api_system_test.rs"]
mod v0_system_test;

// SPEC-a6e55b37: GET /api/system 認証不要テスト (FR-006)
#[path = "integration/system_info_public_test.rs"]
mod system_info_public_test;

// SPEC-62ac4b68: IPアドレスロギング＆クライアント分析
#[path = "integration/settings_test.rs"]
mod settings_test;

// Tests are defined inside the modules; this harness ensures they are built
// and executed when running `cargo test`.

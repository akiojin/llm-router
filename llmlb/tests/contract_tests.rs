//! load balancer contract tests entrypoint

#[path = "support/mod.rs"]
pub mod support;

#[path = "contract/test_proxy_completions.rs"]
mod test_proxy_completions;

// SPEC-e8e9326e: エンドポイント管理API契約テスト
#[path = "contract/endpoints_post_test.rs"]
mod endpoints_post_test;

#[path = "contract/endpoints_get_list_test.rs"]
mod endpoints_get_list_test;

#[path = "contract/endpoints_get_detail_test.rs"]
mod endpoints_get_detail_test;

#[path = "contract/endpoints_put_test.rs"]
mod endpoints_put_test;

#[path = "contract/endpoints_delete_test.rs"]
mod endpoints_delete_test;

#[path = "contract/endpoints_model_delete_test.rs"]
mod endpoints_model_delete_test;

#[path = "contract/endpoints_test_test.rs"]
mod endpoints_test_test;

#[path = "contract/endpoints_sync_test.rs"]
mod endpoints_sync_test;

// SPEC-e8e9326e: エンドポイントタイプ関連の契約テスト
#[path = "contract/endpoints_type_filter_test.rs"]
mod endpoints_type_filter_test;

#[path = "contract/endpoints_download_test.rs"]
mod endpoints_download_test;

#[path = "contract/endpoints_download_progress_test.rs"]
mod endpoints_download_progress_test;

#[path = "contract/endpoints_model_info_test.rs"]
mod endpoints_model_info_test;

#[path = "contract/models_api_test.rs"]
mod models_api_test;

#[path = "contract/chat_modal_embed.rs"]
mod chat_modal_embed;

// NOTE: chat_page_spec.rs は削除されました
// Playground機能はダッシュボード内のエンドポイント別Playgroundに移行 (#playground/:endpointId)

#[path = "contract/audio_transcriptions_test.rs"]
mod audio_transcriptions_test;

#[path = "contract/audio_speech_test.rs"]
mod audio_speech_test;

#[path = "contract/images_generations_test.rs"]
mod images_generations_test;

#[path = "contract/images_edits_test.rs"]
mod images_edits_test;

#[path = "contract/images_variations_test.rs"]
mod images_variations_test;

#[path = "contract/openai_logging_test.rs"]
mod openai_logging_test;

#[path = "contract/openai_request_sanitization_spec.rs"]
mod openai_request_sanitization_spec;

#[path = "contract/queueing_test.rs"]
mod queueing_test;

#[path = "contract/models_source_test.rs"]
mod models_source_test;

// SPEC-fbc50d97: リクエスト/レスポンス履歴API契約テスト
#[path = "contract/request_history_api_test.rs"]
mod request_history_api_test;

// SPEC-0f1de549: Open Responses API契約テスト
#[path = "contract/responses_api_test.rs"]
mod responses_api_test;

#[path = "contract/anthropic_messages_api_test.rs"]
mod anthropic_messages_api_test;

// SPEC-8c32349f: エンドポイント日次統計API契約テスト
#[path = "contract/endpoint_daily_stats_api_test.rs"]
mod endpoint_daily_stats_api_test;

// SPEC-82cd11b7: バージョンAPI契約テスト
#[path = "contract/version_api_test.rs"]
mod version_api_test;

// SPEC-62ac4b68: Clients分析API契約テスト
#[path = "contract/clients_api_test.rs"]
mod clients_api_test;

// SPEC-62ac4b68: Clients時系列・モデル分布API契約テスト
#[path = "contract/clients_timeline_test.rs"]
mod clients_timeline_test;

// SPEC-62ac4b68: Clientsヒートマップ契約テスト
#[path = "contract/clients_heatmap_test.rs"]
mod clients_heatmap_test;

// SPEC-62ac4b68: Clientsドリルダウン契約テスト
#[path = "contract/clients_drilldown_test.rs"]
mod clients_drilldown_test;

// SPEC-62ac4b68: ClientsAPIキー集計契約テスト
#[path = "contract/clients_apikeys_test.rs"]
mod clients_apikeys_test;

// SPEC-62ac4b68: 異常検知（閾値設定・超過検出）契約テスト
#[path = "contract/clients_alert_test.rs"]
mod clients_alert_test;

// Issue #446: ユーザー作成API viewerロール契約テスト
#[path = "contract/users_create_viewer_test.rs"]
mod users_create_viewer_test;

// 認証API契約テスト
#[path = "contract/auth_api_test.rs"]
mod auth_api_test;

// ユーザー管理API契約テスト
#[path = "contract/users_api_test.rs"]
mod users_api_test;

// APIキー管理API契約テスト
#[path = "contract/api_keys_api_test.rs"]
mod api_keys_api_test;

// SPEC-e8e9326e: エンドポイント管理API拡張契約テスト（不足ケース補完）
#[path = "contract/endpoints_contract_extended_test.rs"]
mod endpoints_contract_extended_test;

// SPEC #585 FR-027 / Issue #696: 公開ルートの契約テスト欠落を補完
#[path = "contract/api_route_coverage_test.rs"]
mod api_route_coverage_test;

// Tests are defined inside the modules; this harness ensures they are built
// and executed when running `cargo test`.

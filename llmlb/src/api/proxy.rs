//! LLM runtimeプロキシ APIハンドラー
//!
//! # SPEC-f8e3a1b7: Endpoint型への移行完了
//!
//! このモジュールはEndpoint型を使用しています。

use crate::api::openai_util::classify_upstream_request_error;
use crate::common::{
    error::LbError,
    protocol::{RequestResponseRecord, TpsApiKind},
};
use crate::token::StreamingTokenAccumulator;
use crate::{
    config::QueueConfig,
    types::endpoint::{Endpoint, SupportedAPI},
    AppState,
};
use axum::{
    body::Body,
    http::{HeaderName, HeaderValue, StatusCode},
    response::Response,
};
use futures::{Stream, StreamExt, TryStreamExt};
use std::{io, pin::Pin, sync::Arc, time::Instant};

/// TPS優先でエンドポイントを選択
///
/// llmlbはゲートウェイとしてエンドポイントをブラックボックスとして扱い、
/// TPS（Tokens Per Second）のEMA値が最も高いエンドポイントを優先選択する。
/// TPS未計測のエンドポイントはTPS=0.0として最低優先で扱う。
/// 同一TPS時はラウンドロビンでタイブレークする。
pub(crate) async fn select_available_endpoint(state: &AppState) -> Result<Endpoint, LbError> {
    state.load_manager.select_endpoint_by_tps_direct(None).await
}

/// キュー付きエンドポイント選択の結果
#[allow(dead_code)]
pub(crate) enum QueueSelection {
    /// エンドポイントが見つかった
    Ready {
        endpoint: Box<Endpoint>,
        queued_wait_ms: Option<u128>,
    },
    /// キャパシティ超過
    CapacityExceeded,
    /// タイムアウト
    Timeout { waited_ms: u128 },
}

/// モデル対応のエンドポイントをTPS優先・キュー付きで選択
pub(crate) async fn select_available_endpoint_with_queue_for_model(
    state: &AppState,
    _queue_config: QueueConfig,
    model_id: &str,
    api_kind: Option<TpsApiKind>,
) -> Result<QueueSelection, LbError> {
    let endpoint = state
        .load_manager
        .select_endpoint_by_tps_ready_for_model(model_id, api_kind)
        .await?;

    tracing::debug!(
        model = %model_id,
        endpoint_id = %endpoint.id,
        endpoint_name = %endpoint.name,
        ?api_kind,
        "Selected ready endpoint by TPS priority"
    );

    Ok(QueueSelection::Ready {
        endpoint: Box::new(endpoint),
        queued_wait_ms: None,
    })
}

/// モデルと必須APIに対応するエンドポイントをTPS優先・キュー付きで選択
pub(crate) async fn select_available_endpoint_with_queue_for_model_and_api(
    state: &AppState,
    _queue_config: QueueConfig,
    model_id: &str,
    required_api: SupportedAPI,
    api_kind: Option<TpsApiKind>,
) -> Result<QueueSelection, LbError> {
    let endpoints = state
        .endpoint_registry
        .find_by_model_and_supported_api(model_id, required_api)
        .await;
    if endpoints.is_empty() {
        return Err(LbError::NoCapableEndpoints(model_id.to_string()));
    }

    let endpoint = state
        .load_manager
        .select_endpoint_by_tps_ready_from_candidates(endpoints, model_id, api_kind)
        .await?;

    tracing::debug!(
        model = %model_id,
        endpoint_id = %endpoint.id,
        endpoint_name = %endpoint.name,
        required_api = %required_api,
        ?api_kind,
        "Selected ready endpoint by TPS priority and required API"
    );

    Ok(QueueSelection::Ready {
        endpoint: Box::new(endpoint),
        queued_wait_ms: None,
    })
}

pub(crate) fn forward_streaming_response(response: reqwest::Response) -> Result<Response, LbError> {
    let status = response.status();
    let headers = response.headers().clone();
    let stream = response.bytes_stream().map_err(io::Error::other);
    let body = Body::from_stream(stream);
    let mut axum_response = Response::new(body);
    *axum_response.status_mut() = StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::OK);
    {
        let response_headers = axum_response.headers_mut();
        for (name, value) in headers.iter() {
            if let (Ok(header_name), Ok(header_value)) = (
                HeaderName::from_bytes(name.as_str().as_bytes()),
                HeaderValue::from_bytes(value.as_bytes()),
            ) {
                response_headers.insert(header_name, header_value);
            }
        }
    }
    use axum::http::header;
    if !axum_response
        .headers()
        .get(header::CONTENT_TYPE)
        .map(|v| v.to_str().unwrap_or("").starts_with("text/event-stream"))
        .unwrap_or(false)
    {
        axum_response.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        );
    }
    Ok(axum_response)
}

fn process_sse_lines(
    buffer: &mut String,
    chunk_text: &str,
    accumulator: &mut StreamingTokenAccumulator,
) {
    buffer.push_str(chunk_text);

    while let Some(newline_idx) = buffer.find('\n') {
        let line = buffer[..newline_idx].trim_end_matches('\r').to_string();
        accumulator.process_chunk(&line);
        buffer.drain(..=newline_idx);
    }
}

/// SSEストリームを透過しながら、完了時にTPS計測用のトークンを集計する。
#[allow(clippy::too_many_arguments)]
pub(crate) fn forward_streaming_response_with_tps_tracking(
    response: reqwest::Response,
    endpoint_id: uuid::Uuid,
    model_id: String,
    api_kind: Option<TpsApiKind>,
    endpoint_type: crate::types::endpoint::EndpointType,
    request_started_at: Instant,
    endpoint_registry: crate::registry::endpoints::EndpointRegistry,
    load_manager: crate::balancer::LoadManager,
    event_bus: crate::events::SharedEventBus,
) -> Result<Response, LbError> {
    struct TpsTrackingState {
        upstream: Pin<Box<dyn Stream<Item = Result<axum::body::Bytes, reqwest::Error>> + Send>>,
        accumulator: StreamingTokenAccumulator,
        sse_buffer: String,
        endpoint_id: uuid::Uuid,
        model_id: String,
        api_kind: Option<TpsApiKind>,
        endpoint_type: crate::types::endpoint::EndpointType,
        request_started_at: Instant,
        endpoint_registry: crate::registry::endpoints::EndpointRegistry,
        load_manager: crate::balancer::LoadManager,
        event_bus: crate::events::SharedEventBus,
        stats_recorded: bool,
    }

    impl TpsTrackingState {
        fn finalize_output_tokens_and_duration(&mut self) -> (u64, u64) {
            if !self.sse_buffer.is_empty() {
                let pending = std::mem::take(&mut self.sse_buffer);
                self.accumulator
                    .process_chunk(pending.trim_end_matches('\r'));
            }

            let usage = self.accumulator.finalize();
            let output_tokens = usage.output_tokens.unwrap_or(0) as u64;
            let duration_ms = if output_tokens > 0 {
                self.request_started_at.elapsed().as_millis().max(1) as u64
            } else {
                0
            };

            (output_tokens, duration_ms)
        }

        fn record_stats_once(&mut self, success: bool, output_tokens: u64, duration_ms: u64) {
            if self.stats_recorded {
                return;
            }
            self.stats_recorded = true;

            record_endpoint_request_stats(
                self.endpoint_registry.clone(),
                self.endpoint_id,
                self.model_id.clone(),
                success,
                output_tokens,
                duration_ms,
                self.api_kind,
                self.endpoint_type,
                self.load_manager.clone(),
                self.event_bus.clone(),
            );
        }
    }

    impl Drop for TpsTrackingState {
        fn drop(&mut self) {
            if self.stats_recorded {
                return;
            }

            let (output_tokens, duration_ms) = self.finalize_output_tokens_and_duration();

            if tokio::runtime::Handle::try_current().is_ok() {
                self.record_stats_once(true, output_tokens, duration_ms);
            } else {
                tracing::warn!(
                    endpoint_id = %self.endpoint_id,
                    model_id = %self.model_id,
                    "Streaming TPS tracker dropped without runtime; skipping stats fallback"
                );
            }
        }
    }

    let status = response.status();
    let headers = response.headers().clone();

    let state = TpsTrackingState {
        upstream: Box::pin(response.bytes_stream()),
        accumulator: StreamingTokenAccumulator::new(&model_id),
        sse_buffer: String::new(),
        endpoint_id,
        model_id,
        api_kind,
        endpoint_type,
        request_started_at,
        endpoint_registry,
        load_manager,
        event_bus,
        stats_recorded: false,
    };

    let tracked_stream = futures::stream::try_unfold(state, |mut state| async move {
        match state.upstream.next().await {
            Some(Ok(chunk)) => {
                let chunk_text = String::from_utf8_lossy(chunk.as_ref());
                process_sse_lines(&mut state.sse_buffer, &chunk_text, &mut state.accumulator);
                Ok(Some((chunk, state)))
            }
            Some(Err(err)) => {
                state.record_stats_once(false, 0, 0);
                Err(io::Error::other(err))
            }
            None => {
                let (output_tokens, duration_ms) = state.finalize_output_tokens_and_duration();
                state.record_stats_once(true, output_tokens, duration_ms);
                Ok(None)
            }
        }
    });

    let body = Body::from_stream(tracked_stream);
    let mut axum_response = Response::new(body);
    *axum_response.status_mut() = StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::OK);
    {
        let response_headers = axum_response.headers_mut();
        for (name, value) in headers.iter() {
            if let (Ok(header_name), Ok(header_value)) = (
                HeaderName::from_bytes(name.as_str().as_bytes()),
                HeaderValue::from_bytes(value.as_bytes()),
            ) {
                response_headers.insert(header_name, header_value);
            }
        }
    }
    use axum::http::header;
    if !axum_response
        .headers()
        .get(header::CONTENT_TYPE)
        .map(|v| v.to_str().unwrap_or("").starts_with("text/event-stream"))
        .unwrap_or(false)
    {
        axum_response.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        );
    }
    Ok(axum_response)
}

/// リクエスト/レスポンスレコードを保存（Fire-and-forget）
pub(crate) fn save_request_record(
    storage: Arc<crate::db::request_history::RequestHistoryStorage>,
    record: RequestResponseRecord,
) {
    tokio::spawn(async move {
        if let Err(e) = storage.save_record(&record).await {
            tracing::error!("Failed to save request record: {}", e);
        }
    });
}

/// エンドポイントリクエスト統計を更新（Fire-and-forget）（SPEC-8c32349f）
///
/// endpointsテーブルの累計カウンタとendpoint_daily_statsの日次集計を
/// 非同期で更新する。リクエスト処理のレイテンシに影響を与えない。
/// SPEC-4bb5b55f: TPS計測対象の場合はインメモリEMAも更新する。
#[allow(clippy::too_many_arguments)]
pub(crate) fn record_endpoint_request_stats(
    endpoint_registry: crate::registry::endpoints::EndpointRegistry,
    endpoint_id: uuid::Uuid,
    model_id: String,
    success: bool,
    output_tokens: u64,
    duration_ms: u64,
    api_kind: Option<TpsApiKind>,
    endpoint_type: crate::types::endpoint::EndpointType,
    load_manager: crate::balancer::LoadManager,
    event_bus: crate::events::SharedEventBus,
) {
    tokio::spawn(async move {
        let date = chrono::Local::now().format("%Y-%m-%d").to_string();
        let pool = endpoint_registry.pool().clone();

        if let Err(e) = endpoint_registry
            .increment_request_counters(endpoint_id, success)
            .await
        {
            tracing::error!("Failed to increment endpoint request counters: {}", e);
        }

        // TPS計測対象かつ成功かつ有効トークンがある場合のみトークン・時間をDB永続化
        let should_update_tps = endpoint_type.is_tps_trackable()
            && api_kind.is_some()
            && success
            && output_tokens > 0
            && duration_ms > 0;
        let (tokens, duration) = if should_update_tps {
            (output_tokens, duration_ms)
        } else {
            (0, 0)
        };

        let api_kind_str = api_kind
            .and_then(|k| serde_json::to_value(k).ok())
            .and_then(|v| v.as_str().map(String::from))
            .unwrap_or_else(|| "chat_completions".to_string());

        if let Err(e) = crate::db::endpoint_daily_stats::upsert_daily_stats_with_api_kind(
            &pool,
            endpoint_id,
            &model_id,
            &date,
            &api_kind_str,
            success,
            tokens,
            duration,
        )
        .await
        {
            tracing::error!("Failed to upsert daily stats: {}", e);
        }

        // SPEC-4bb5b55f: インメモリTPS EMAを更新 & イベント発行
        if should_update_tps {
            let api_kind = api_kind.expect("checked above");
            load_manager
                .update_tps(
                    endpoint_id,
                    model_id.clone(),
                    api_kind,
                    output_tokens,
                    duration_ms,
                )
                .await;

            let tps = output_tokens as f64 / (duration_ms as f64 / 1000.0);
            event_bus.publish(crate::events::DashboardEvent::TpsUpdated {
                endpoint_id,
                model_id,
                tps,
                output_tokens: output_tokens as u32,
                duration_ms,
            });
        }
    });
}
/// エンドポイントにリクエストを転送
///
/// OpenAI互換APIエンドポイントにリクエストを転送し、レスポンスを返す
pub(crate) async fn forward_to_endpoint(
    client: &reqwest::Client,
    endpoint: &Endpoint,
    path: &str,
    body: Vec<u8>,
    stream: bool,
) -> Result<reqwest::Response, LbError> {
    let url = format!("{}{}", endpoint.base_url.trim_end_matches('/'), path);

    let mut request_builder = client
        .post(&url)
        .header("Content-Type", "application/json")
        .timeout(std::time::Duration::from_secs(
            endpoint.inference_timeout_secs as u64,
        ))
        .body(body);

    // APIキーがあれば追加
    if let Some(api_key) = &endpoint.api_key {
        request_builder = request_builder.bearer_auth(api_key);
    }

    let response = request_builder.send().await.map_err(|e| {
        tracing::error!(
            "Failed to forward request to endpoint {}: {}",
            endpoint.name,
            e
        );
        let classified =
            classify_upstream_request_error(&e, &url, endpoint.inference_timeout_secs, None);
        if classified.status_code == StatusCode::GATEWAY_TIMEOUT {
            LbError::Timeout(classified.record_message)
        } else {
            LbError::Http(classified.record_message)
        }
    })?;

    // エラーステータスをチェック
    let status = response.status();
    if !status.is_success() && !stream {
        // 非ストリーミングの場合はエラー内容を取得してログ
        let error_body = match response.text().await {
            Ok(body) => body,
            Err(e) => {
                tracing::debug!(
                    "Failed to read error body from endpoint {}: {}",
                    endpoint.name,
                    e
                );
                String::new()
            }
        };
        tracing::warn!(
            "Endpoint {} returned error {}: {}",
            endpoint.name,
            status,
            error_body
        );
        return Err(LbError::Http(format!(
            "Endpoint returned {}: {}",
            status, error_body
        )));
    }

    Ok(response)
}

// NOTE: テストはNodeRegistry廃止に伴い削除されました。
// 新しいテストはEndpointRegistryベースで tests/integration/ に追加してください。

#[cfg(test)]
mod tests {
    use super::*;
    use crate::token::StreamingTokenAccumulator;

    // --- QueueSelection enum ---

    #[test]
    fn queue_selection_ready_variant_holds_endpoint_and_wait() {
        let ep = Endpoint::new(
            "ep1".to_string(),
            "http://localhost:8080".to_string(),
            crate::types::endpoint::EndpointType::Xllm,
        );
        let qs = QueueSelection::Ready {
            endpoint: Box::new(ep),
            queued_wait_ms: Some(42),
        };
        if let QueueSelection::Ready {
            endpoint,
            queued_wait_ms,
        } = qs
        {
            assert_eq!(endpoint.name, "ep1");
            assert_eq!(queued_wait_ms, Some(42));
        } else {
            panic!("Expected QueueSelection::Ready");
        }
    }

    #[test]
    fn queue_selection_ready_variant_no_wait() {
        let ep = Endpoint::new(
            "ep2".to_string(),
            "http://localhost:8081".to_string(),
            crate::types::endpoint::EndpointType::Ollama,
        );
        let qs = QueueSelection::Ready {
            endpoint: Box::new(ep),
            queued_wait_ms: None,
        };
        if let QueueSelection::Ready { queued_wait_ms, .. } = qs {
            assert!(queued_wait_ms.is_none());
        } else {
            panic!("Expected QueueSelection::Ready");
        }
    }

    #[test]
    fn queue_selection_capacity_exceeded_variant() {
        let qs = QueueSelection::CapacityExceeded;
        assert!(matches!(qs, QueueSelection::CapacityExceeded));
    }

    #[test]
    fn queue_selection_timeout_variant() {
        let qs = QueueSelection::Timeout { waited_ms: 5000 };
        if let QueueSelection::Timeout { waited_ms } = qs {
            assert_eq!(waited_ms, 5000);
        } else {
            panic!("Expected QueueSelection::Timeout");
        }
    }

    #[test]
    fn queue_selection_timeout_zero() {
        let qs = QueueSelection::Timeout { waited_ms: 0 };
        if let QueueSelection::Timeout { waited_ms } = qs {
            assert_eq!(waited_ms, 0);
        } else {
            panic!("Expected QueueSelection::Timeout");
        }
    }

    // --- process_sse_lines ---

    #[test]
    fn process_sse_lines_empty_chunk() {
        let mut buffer = String::new();
        let mut acc = StreamingTokenAccumulator::new("test-model");
        process_sse_lines(&mut buffer, "", &mut acc);
        assert!(buffer.is_empty());
    }

    #[test]
    fn process_sse_lines_single_line_with_newline() {
        let mut buffer = String::new();
        let mut acc = StreamingTokenAccumulator::new("test-model");
        process_sse_lines(
            &mut buffer,
            "data: {\"choices\":[{\"delta\":{\"content\":\"hello\"}}]}\n",
            &mut acc,
        );
        assert!(buffer.is_empty());
    }

    #[test]
    fn process_sse_lines_partial_line_no_newline() {
        let mut buffer = String::new();
        let mut acc = StreamingTokenAccumulator::new("test-model");
        process_sse_lines(&mut buffer, "data: partial", &mut acc);
        assert_eq!(buffer, "data: partial");
    }

    #[test]
    fn process_sse_lines_multiple_lines_in_one_chunk() {
        let mut buffer = String::new();
        let mut acc = StreamingTokenAccumulator::new("test-model");
        process_sse_lines(&mut buffer, "data: line1\ndata: line2\n", &mut acc);
        assert!(buffer.is_empty());
    }

    #[test]
    fn process_sse_lines_split_across_chunks() {
        let mut buffer = String::new();
        let mut acc = StreamingTokenAccumulator::new("test-model");

        // First chunk: partial line
        process_sse_lines(&mut buffer, "data: hel", &mut acc);
        assert_eq!(buffer, "data: hel");

        // Second chunk: rest of line
        process_sse_lines(&mut buffer, "lo\n", &mut acc);
        assert!(buffer.is_empty());
    }

    #[test]
    fn process_sse_lines_carriage_return_stripped() {
        let mut buffer = String::new();
        let mut acc = StreamingTokenAccumulator::new("test-model");
        process_sse_lines(&mut buffer, "data: test\r\n", &mut acc);
        assert!(buffer.is_empty());
    }

    #[test]
    fn process_sse_lines_done_marker() {
        let mut buffer = String::new();
        let mut acc = StreamingTokenAccumulator::new("test-model");
        process_sse_lines(&mut buffer, "data: [DONE]\n", &mut acc);
        assert!(buffer.is_empty());
    }

    #[test]
    fn process_sse_lines_buffer_accumulates_partial() {
        let mut buffer = String::new();
        let mut acc = StreamingTokenAccumulator::new("test-model");

        process_sse_lines(&mut buffer, "abc", &mut acc);
        assert_eq!(buffer, "abc");

        process_sse_lines(&mut buffer, "def", &mut acc);
        assert_eq!(buffer, "abcdef");

        process_sse_lines(&mut buffer, "\n", &mut acc);
        assert!(buffer.is_empty());
    }

    #[test]
    fn process_sse_lines_empty_lines() {
        let mut buffer = String::new();
        let mut acc = StreamingTokenAccumulator::new("test-model");
        process_sse_lines(&mut buffer, "\n\n\n", &mut acc);
        assert!(buffer.is_empty());
    }

    #[test]
    fn process_sse_lines_mixed_content() {
        let mut buffer = String::new();
        let mut acc = StreamingTokenAccumulator::new("test-model");
        process_sse_lines(
            &mut buffer,
            "data: {\"choices\":[{\"delta\":{\"content\":\"A\"}}]}\n\ndata: {\"choices\":[{\"delta\":{\"content\":\"B\"}}]}\nremaining",
            &mut acc,
        );
        assert_eq!(buffer, "remaining");
    }

    // --- forward_to_endpoint URL construction ---

    #[test]
    fn forward_url_trims_trailing_slash() {
        let ep = Endpoint::new(
            "ep".to_string(),
            "http://localhost:8080/".to_string(),
            crate::types::endpoint::EndpointType::Xllm,
        );
        let url = format!(
            "{}{}",
            ep.base_url.trim_end_matches('/'),
            "/v1/chat/completions"
        );
        assert_eq!(url, "http://localhost:8080/v1/chat/completions");
    }

    #[test]
    fn forward_url_no_trailing_slash() {
        let ep = Endpoint::new(
            "ep".to_string(),
            "http://10.0.0.1:11434".to_string(),
            crate::types::endpoint::EndpointType::Ollama,
        );
        let url = format!("{}{}", ep.base_url.trim_end_matches('/'), "/v1/completions");
        assert_eq!(url, "http://10.0.0.1:11434/v1/completions");
    }

    #[test]
    fn forward_url_multiple_trailing_slashes() {
        let base_url = "http://localhost:8080///";
        let url = format!("{}{}", base_url.trim_end_matches('/'), "/v1/embeddings");
        assert_eq!(url, "http://localhost:8080/v1/embeddings");
    }

    // --- Endpoint bearer_auth logic ---

    #[test]
    fn endpoint_with_api_key_has_some() {
        let mut ep = Endpoint::new(
            "ep".to_string(),
            "http://localhost:8080".to_string(),
            crate::types::endpoint::EndpointType::OpenaiCompatible,
        );
        ep.api_key = Some("sk-test-key".to_string());
        assert!(ep.api_key.is_some());
    }

    #[test]
    fn endpoint_without_api_key_has_none() {
        let ep = Endpoint::new(
            "ep".to_string(),
            "http://localhost:8080".to_string(),
            crate::types::endpoint::EndpointType::OpenaiCompatible,
        );
        assert!(ep.api_key.is_none());
    }

    // --- LbError::Http construction from forward_to_endpoint ---

    #[test]
    fn lb_error_http_format() {
        let err = LbError::Http("Endpoint request failed: connection refused".to_string());
        assert!(err.to_string().contains("Endpoint request failed"));
    }

    #[test]
    fn lb_error_http_endpoint_returned_error() {
        let err = LbError::Http("Endpoint returned 500: Internal Server Error".to_string());
        assert!(err.to_string().contains("500"));
    }

    // --- forward_streaming_response content-type behavior ---

    #[tokio::test]
    async fn forward_streaming_response_sets_json_content_type() {
        // Create a minimal reqwest response
        let response = axum::http::Response::builder()
            .status(200)
            .header("x-custom", "test")
            .body("test body")
            .unwrap();
        let reqwest_response = reqwest::Response::from(response);

        let axum_response = forward_streaming_response(reqwest_response).unwrap();
        assert_eq!(axum_response.status(), StatusCode::OK);
        assert_eq!(
            axum_response
                .headers()
                .get("content-type")
                .unwrap()
                .to_str()
                .unwrap(),
            "application/json"
        );
    }

    #[tokio::test]
    async fn forward_streaming_response_preserves_sse_content_type() {
        let response = axum::http::Response::builder()
            .status(200)
            .header("content-type", "text/event-stream")
            .body("data: test\n\n")
            .unwrap();
        let reqwest_response = reqwest::Response::from(response);

        let axum_response = forward_streaming_response(reqwest_response).unwrap();
        assert_eq!(
            axum_response
                .headers()
                .get("content-type")
                .unwrap()
                .to_str()
                .unwrap(),
            "text/event-stream"
        );
    }

    #[tokio::test]
    async fn forward_streaming_response_maps_status_code() {
        let response = axum::http::Response::builder()
            .status(201)
            .body("")
            .unwrap();
        let reqwest_response = reqwest::Response::from(response);

        let axum_response = forward_streaming_response(reqwest_response).unwrap();
        assert_eq!(axum_response.status(), StatusCode::CREATED);
    }

    #[tokio::test]
    async fn forward_streaming_response_maps_error_status() {
        let response = axum::http::Response::builder()
            .status(500)
            .body("")
            .unwrap();
        let reqwest_response = reqwest::Response::from(response);

        let axum_response = forward_streaming_response(reqwest_response).unwrap();
        assert_eq!(axum_response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn forward_streaming_response_preserves_custom_headers() {
        let response = axum::http::Response::builder()
            .status(200)
            .header("x-request-id", "abc123")
            .body("")
            .unwrap();
        let reqwest_response = reqwest::Response::from(response);

        let axum_response = forward_streaming_response(reqwest_response).unwrap();
        assert_eq!(
            axum_response
                .headers()
                .get("x-request-id")
                .unwrap()
                .to_str()
                .unwrap(),
            "abc123"
        );
    }

    #[tokio::test]
    async fn forward_streaming_response_maps_404() {
        let response = axum::http::Response::builder()
            .status(404)
            .body("")
            .unwrap();
        let reqwest_response = reqwest::Response::from(response);

        let axum_response = forward_streaming_response(reqwest_response).unwrap();
        assert_eq!(axum_response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn forward_streaming_response_maps_429() {
        let response = axum::http::Response::builder()
            .status(429)
            .body("")
            .unwrap();
        let reqwest_response = reqwest::Response::from(response);

        let axum_response = forward_streaming_response(reqwest_response).unwrap();
        assert_eq!(axum_response.status(), StatusCode::TOO_MANY_REQUESTS);
    }

    // --- Timeout configuration in forward_to_endpoint ---

    #[test]
    fn endpoint_inference_timeout_default() {
        let ep = Endpoint::new(
            "ep".to_string(),
            "http://localhost:8080".to_string(),
            crate::types::endpoint::EndpointType::Xllm,
        );
        assert_eq!(ep.inference_timeout_secs, 600);
    }

    #[test]
    fn endpoint_inference_timeout_custom() {
        let mut ep = Endpoint::new(
            "ep".to_string(),
            "http://localhost:8080".to_string(),
            crate::types::endpoint::EndpointType::Xllm,
        );
        ep.inference_timeout_secs = 60;
        assert_eq!(
            std::time::Duration::from_secs(ep.inference_timeout_secs as u64),
            std::time::Duration::from_secs(60)
        );
    }
}

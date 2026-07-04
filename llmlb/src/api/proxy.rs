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

/// キュー付きエンドポイント選択の結果
pub(crate) enum QueueSelection {
    /// エンドポイントが見つかった
    Ready {
        endpoint: Box<Endpoint>,
        queued_wait_ms: Option<u128>,
    },
}

/// モデル対応のエンドポイントをTPS優先・キュー付きで選択
pub(crate) async fn select_available_endpoint_with_queue_for_model(
    state: &AppState,
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
        // process_chunk は先頭で trim() するため \r 除去・String 確保は不要（借用のまま渡す）。
        accumulator.process_chunk(&buffer[..newline_idx]);
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
    request_lease: Option<crate::balancer::RequestLease>,
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
        // ストリーム完走時に実時間で完了する RequestLease。
        // ヘッダー受信時点で早期解放すると、ストリーミング中の assigned_active が
        // 過小になり、レイテンシも過小評価され、idle 通知も早まる（lease-ttfb）。
        request_lease: Option<crate::balancer::RequestLease>,
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

        /// ストリーム終端で RequestLease を実時間で完了する。
        ///
        /// 成功時はストリーム完走までの実時間で推論レイテンシも更新する
        /// （ヘッダー受信時点の過小なレイテンシではなく）。
        /// 明示的に完了しなかった場合（クライアント切断等）は lease の Drop が
        /// Error 扱いで自動完了するため、カウンタ残留は起きない。
        async fn finish_lease(&mut self, outcome: crate::balancer::RequestOutcome) {
            let full = self.request_started_at.elapsed();
            if matches!(outcome, crate::balancer::RequestOutcome::Success) {
                if let Err(err) = self
                    .endpoint_registry
                    .update_inference_latency(self.endpoint_id, full.as_millis() as f64)
                    .await
                {
                    tracing::debug!(
                        endpoint_id = %self.endpoint_id,
                        error = %err,
                        "Failed to update inference latency at stream end"
                    );
                }
            }
            if let Some(lease) = self.request_lease.take() {
                if let Err(err) = lease.complete(outcome, full).await {
                    tracing::warn!(
                        endpoint_id = %self.endpoint_id,
                        error = %err,
                        "Failed to complete streaming request lease"
                    );
                }
            }
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
        request_lease,
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
                state
                    .finish_lease(crate::balancer::RequestOutcome::Error)
                    .await;
                Err(io::Error::other(err))
            }
            None => {
                let (output_tokens, duration_ms) = state.finalize_output_tokens_and_duration();
                state.record_stats_once(true, output_tokens, duration_ms);
                state
                    .finish_lease(crate::balancer::RequestOutcome::Success)
                    .await;
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

        // TPS計測対象かつ成功かつ有効トークンがある場合のみトークン・時間をDB永続化。
        // arch-review [L15]: is_tps_trackable() は現状全タイプで true（SPEC-4bb5b55f の拡張点）。
        // 特定タイプを計測対象外にする将来仕様のためのガードとして保持している。
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

    // 上流のレスポンスは非2xxでもそのまま返し、ステータス処理は呼び出し側に委ねる。
    // （全呼び出し元が上流のステータス/本文を保持して扱うため、ここでの非2xx→Err化は不要）
    request_builder.send().await.map_err(|e| {
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
    })
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
        let QueueSelection::Ready {
            endpoint,
            queued_wait_ms,
        } = qs;
        assert_eq!(endpoint.name, "ep1");
        assert_eq!(queued_wait_ms, Some(42));
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
        let QueueSelection::Ready { queued_wait_ms, .. } = qs;
        assert!(queued_wait_ms.is_none());
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

    // --- lease-ttfb: ストリーミング forwarder は配信完了まで lease を保持する ---

    #[tokio::test]
    async fn streaming_forwarder_holds_lease_until_stream_consumed() {
        let pool = sqlx::SqlitePool::connect("sqlite::memory:")
            .await
            .expect("create test db");
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .expect("run migrations");
        let registry = crate::registry::endpoints::EndpointRegistry::new(pool)
            .await
            .expect("create registry");
        let endpoint = Endpoint::new(
            "lease-stream-ep".to_string(),
            "http://localhost:1".to_string(),
            crate::types::endpoint::EndpointType::OpenaiCompatible,
        );
        let endpoint_id = endpoint.id;
        registry.add(endpoint).await.expect("add endpoint");
        let load_manager = crate::balancer::LoadManager::new(Arc::new(registry.clone()));
        let event_bus = crate::events::create_shared_event_bus();

        let lease = load_manager
            .begin_request(endpoint_id)
            .await
            .expect("begin request");
        assert_eq!(
            load_manager
                .snapshot(endpoint_id)
                .await
                .unwrap()
                .active_requests,
            1,
            "begin_request must mark one active request"
        );

        let response = reqwest::Response::from(
            axum::http::Response::builder()
                .status(200)
                .header("content-type", "text/event-stream")
                .body("data: {\"choices\":[]}\n\ndata: [DONE]\n\n")
                .unwrap(),
        );

        let axum_response = forward_streaming_response_with_tps_tracking(
            response,
            endpoint_id,
            "m".to_string(),
            None,
            crate::types::endpoint::EndpointType::OpenaiCompatible,
            Instant::now(),
            registry,
            load_manager.clone(),
            event_bus,
            Some(lease),
        )
        .expect("forward streaming");

        // ストリーム未消費の段階では lease は保持されたまま（早期解放しない）
        assert_eq!(
            load_manager
                .snapshot(endpoint_id)
                .await
                .unwrap()
                .active_requests,
            1,
            "lease must be held until the stream is consumed"
        );

        // ボディを完全に消費するとストリーム終端で lease が完了する
        let _ = axum::body::to_bytes(axum_response.into_body(), usize::MAX)
            .await
            .expect("consume body");

        assert_eq!(
            load_manager
                .snapshot(endpoint_id)
                .await
                .unwrap()
                .active_requests,
            0,
            "lease must be completed after the stream is fully consumed"
        );
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

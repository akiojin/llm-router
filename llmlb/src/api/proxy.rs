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

/// SPEC-f8e3a1b7: 推論リクエスト成功時にエンドポイントのレイテンシを更新（Fire-and-forget）
pub(crate) fn update_inference_latency(
    registry: &crate::registry::endpoints::EndpointRegistry,
    endpoint_id: uuid::Uuid,
    duration: std::time::Duration,
) {
    let registry = registry.clone();
    let latency_ms = duration.as_millis() as f64;
    tokio::spawn(async move {
        if let Err(e) = registry
            .update_inference_latency(endpoint_id, latency_ms)
            .await
        {
            tracing::debug!(
                endpoint_id = %endpoint_id,
                latency_ms = latency_ms,
                error = %e,
                "Failed to update inference latency"
            );
        }
    });
}

/// キュー待機情報を示す HTTP ヘッダをレスポンスに付与する。
pub(crate) fn add_queue_headers(response: &mut Response, wait_ms: u128) {
    let headers = response.headers_mut();
    headers.insert(
        HeaderName::from_static("x-queue-status"),
        HeaderValue::from_static("queued"),
    );
    let wait_value = wait_ms.to_string();
    if let Ok(value) = HeaderValue::from_str(&wait_value) {
        headers.insert(HeaderName::from_static("x-queue-wait-ms"), value);
    }
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
mod tests;

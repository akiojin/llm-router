//! OpenAI ストリーミング SSE → Anthropic ネイティブ SSE イベント変換
//!
//! arch-review [H6]: api/anthropic.rs から、ステートフルなストリーム変換
//! （message_start/content_block/delta/stop、tool_call 蓄積、統計フォールバック）を分離。

use super::response::map_finish_reason_to_stop_reason;
use crate::api::proxy::record_endpoint_request_stats;
use crate::common::protocol::TpsApiKind;
use crate::token::{StreamingTokenAccumulator, TokenUsage};
use axum::body::{Body, Bytes};
use axum::http::{header, HeaderName, HeaderValue, StatusCode};
use axum::response::Response;
use futures::{Stream, StreamExt};
use serde_json::{json, Value};
use std::collections::{BTreeMap, VecDeque};
use std::io;
use std::pin::Pin;
use std::time::Instant;
use uuid::Uuid;

/// ストリーミング中に断片化された tool_call を index 単位で蓄積する
#[derive(Default)]
pub(super) struct StreamingToolCall {
    id: Option<String>,
    name: Option<String>,
    arguments: String,
}

pub(super) struct AnthropicStreamTracker {
    pub(super) upstream: Pin<Box<dyn Stream<Item = Result<Bytes, reqwest::Error>> + Send>>,
    pub(super) upstream_line_buffer: String,
    pub(super) output_queue: VecDeque<Bytes>,
    pub(super) accumulator: StreamingTokenAccumulator,
    pub(super) endpoint_id: Uuid,
    pub(super) model_id: String,
    pub(super) endpoint_type: crate::types::endpoint::EndpointType,
    pub(super) request_started_at: Instant,
    pub(super) endpoint_registry: crate::registry::endpoints::EndpointRegistry,
    pub(super) load_manager: crate::balancer::LoadManager,
    pub(super) event_bus: crate::events::SharedEventBus,
    pub(super) sent_message_start: bool,
    pub(super) sent_message_stop: bool,
    /// 次に割り当てる content block index（0始まり連番）
    pub(super) next_block_index: usize,
    /// 現在開いているテキストブロックの index（無ければ None）
    pub(super) open_text_index: Option<usize>,
    /// tool_call を index 単位で蓄積し、終了時に単一オープンで逐次出力する
    pub(super) tool_buffer: BTreeMap<u64, StreamingToolCall>,
    /// tool_use ブロックを1つ以上出力したか（stop_reason 補正用）
    pub(super) emitted_tool_use: bool,
    pub(super) response_id: String,
    pub(super) public_model: String,
    pub(super) stop_reason: Option<&'static str>,
    pub(super) stop_sequence: Option<String>,
    pub(super) stats_recorded: bool,
}

#[allow(clippy::too_many_arguments)]
pub(super) fn transform_openai_streaming_response_to_anthropic(
    response: reqwest::Response,
    endpoint_id: Uuid,
    model_id: String,
    endpoint_type: crate::types::endpoint::EndpointType,
    request_started_at: Instant,
    input_tokens: Option<u32>,
    endpoint_registry: crate::registry::endpoints::EndpointRegistry,
    load_manager: crate::balancer::LoadManager,
    event_bus: crate::events::SharedEventBus,
) -> Response {
    let headers = response.headers().clone();
    let mut accumulator = StreamingTokenAccumulator::new(&model_id);
    accumulator.set_input_tokens(input_tokens);

    let state = AnthropicStreamTracker {
        upstream: Box::pin(response.bytes_stream()),
        upstream_line_buffer: String::new(),
        output_queue: VecDeque::new(),
        accumulator,
        endpoint_id,
        model_id: model_id.clone(),
        endpoint_type,
        request_started_at,
        endpoint_registry,
        load_manager,
        event_bus,
        sent_message_start: false,
        sent_message_stop: false,
        next_block_index: 0,
        open_text_index: None,
        tool_buffer: BTreeMap::new(),
        emitted_tool_use: false,
        response_id: format!("msg_{}", Uuid::new_v4().simple()),
        public_model: model_id,
        stop_reason: None,
        stop_sequence: None,
        stats_recorded: false,
    };

    let transformed_stream = futures::stream::try_unfold(state, |mut state| async move {
        loop {
            if let Some(chunk) = state.output_queue.pop_front() {
                return Ok(Some((chunk, state)));
            }

            match state.upstream.next().await {
                Some(Ok(chunk)) => {
                    let chunk_text = String::from_utf8_lossy(chunk.as_ref());
                    state.process_upstream_chunk(&chunk_text);
                }
                Some(Err(err)) => {
                    state.record_stats_once(false, TokenUsage::new(None, Some(0), Some(0)));
                    return Err(io::Error::other(err));
                }
                None => {
                    if !state.sent_message_stop {
                        state.finish_stream();
                        continue;
                    }
                    state.record_stats_once(true, state.accumulator.finalize());
                    return Ok(None);
                }
            }
        }
    });

    let mut response = Response::new(Body::from_stream(transformed_stream));
    *response.status_mut() = StatusCode::OK;
    for (name, value) in headers.iter() {
        if name == reqwest::header::CONTENT_LENGTH {
            continue;
        }
        if let (Ok(header_name), Ok(header_value)) = (
            HeaderName::from_bytes(name.as_str().as_bytes()),
            HeaderValue::from_bytes(value.as_bytes()),
        ) {
            response.headers_mut().insert(header_name, header_value);
        }
    }
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/event-stream"),
    );
    response
}

impl AnthropicStreamTracker {
    fn process_upstream_chunk(&mut self, chunk_text: &str) {
        self.upstream_line_buffer.push_str(chunk_text);

        while let Some(newline_idx) = self.upstream_line_buffer.find('\n') {
            let line = self.upstream_line_buffer[..newline_idx]
                .trim_end_matches('\r')
                .to_string();
            self.upstream_line_buffer.drain(..=newline_idx);
            self.process_upstream_line(&line);
        }
    }

    pub(super) fn process_upstream_line(&mut self, line: &str) {
        // メッセージ確定後は一切のイベントを発行しない（[DONE] 後に届くデータ等）
        if self.sent_message_stop {
            return;
        }
        self.accumulator.process_chunk(line);

        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with(':') {
            return;
        }
        let Some(data) = trimmed.strip_prefix("data:") else {
            return;
        };
        let data = data.trim();

        if data == "[DONE]" {
            self.finish_stream();
            return;
        }

        let Ok(json) = serde_json::from_str::<Value>(data) else {
            return;
        };

        if let Some(id) = json.get("id").and_then(Value::as_str) {
            self.response_id = id.replace("chatcmpl-", "msg_").replace("chatcmpl", "msg");
        }

        // upstream のエラーチャンクを握り潰さず Anthropic の error イベントとして配信する
        if let Some(error_obj) = json.get("error") {
            self.ensure_message_start();
            let message = error_obj
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("upstream error")
                .to_string();
            self.emit_event(
                "error",
                json!({
                    "type": "error",
                    "error": { "type": "api_error", "message": message }
                }),
            );
            let usage = self.accumulator.finalize();
            self.record_stats_once(false, usage);
            self.sent_message_stop = true;
            return;
        }

        self.ensure_message_start();

        if let Some(choice) = json
            .get("choices")
            .and_then(Value::as_array)
            .and_then(|choices| choices.first())
        {
            let delta = choice.get("delta");

            if let Some(content) = delta
                .and_then(|delta| delta.get("content"))
                .and_then(Value::as_str)
            {
                let text_index = self.ensure_text_block_open();
                if !content.is_empty() {
                    self.emit_event(
                        "content_block_delta",
                        json!({
                            "type": "content_block_delta",
                            "index": text_index,
                            "delta": {
                                "type": "text_delta",
                                "text": content
                            }
                        }),
                    );
                }
            }

            if let Some(tool_calls) = delta
                .and_then(|d| d.get("tool_calls"))
                .and_then(Value::as_array)
            {
                if !tool_calls.is_empty() {
                    // 開いているテキストブロックを閉じてから tool を蓄積する。
                    // Anthropic の「同時に開く content block は1つ」契約を守るため、
                    // tool_use ブロックは finish_stream で単一オープンに逐次出力する。
                    if let Some(text_index) = self.open_text_index.take() {
                        self.emit_event(
                            "content_block_stop",
                            json!({ "type": "content_block_stop", "index": text_index }),
                        );
                    }

                    // tool_callはデルタ間で断片化される（最初のデルタにid+name、後続に引数の断片）。
                    // index 単位で id/name/arguments を蓄積する（interleave/再出現にも頑健）。
                    for tool_call in tool_calls {
                        let oai_index = tool_call.get("index").and_then(Value::as_u64).unwrap_or(0);
                        let func = tool_call.get("function");
                        let entry = self.tool_buffer.entry(oai_index).or_default();
                        if entry.id.is_none() {
                            if let Some(id) = tool_call.get("id").and_then(Value::as_str) {
                                entry.id = Some(id.to_string());
                            }
                        }
                        if entry.name.is_none() {
                            if let Some(name) =
                                func.and_then(|f| f.get("name")).and_then(Value::as_str)
                            {
                                entry.name = Some(name.to_string());
                            }
                        }
                        // arguments は文字列・オブジェクトいずれの形でも取りこぼさない
                        match func.and_then(|f| f.get("arguments")) {
                            Some(Value::String(s)) => entry.arguments.push_str(s),
                            Some(v) if !v.is_null() => entry.arguments.push_str(&v.to_string()),
                            _ => {}
                        }
                    }
                }
            }

            if let Some(finish_reason) = choice.get("finish_reason").and_then(Value::as_str) {
                self.stop_reason = Some(map_finish_reason_to_stop_reason(finish_reason));
            }
        }
    }

    fn ensure_message_start(&mut self) {
        if self.sent_message_start {
            return;
        }
        self.sent_message_start = true;
        self.emit_event(
            "message_start",
            json!({
                "type": "message_start",
                "message": {
                    "id": self.response_id,
                    "type": "message",
                    "role": "assistant",
                    "content": [],
                    "model": self.public_model,
                    "stop_reason": null,
                    "stop_sequence": null,
                    "usage": {
                        "input_tokens": self.accumulator.finalize().input_tokens.unwrap_or(0),
                        "output_tokens": 0
                    }
                }
            }),
        );
    }

    /// テキストブロックを開く（未オープンなら次の連番 index を割り当てて開始）。開いている index を返す。
    fn ensure_text_block_open(&mut self) -> usize {
        if let Some(idx) = self.open_text_index {
            return idx;
        }
        let idx = self.next_block_index;
        self.next_block_index += 1;
        self.open_text_index = Some(idx);
        self.emit_event(
            "content_block_start",
            json!({
                "type": "content_block_start",
                "index": idx,
                "content_block": {
                    "type": "text",
                    "text": ""
                }
            }),
        );
        idx
    }

    pub(super) fn finish_stream(&mut self) {
        if self.sent_message_stop {
            return;
        }

        self.ensure_message_start();

        // 開いているテキストブロックを閉じる
        if let Some(text_index) = self.open_text_index.take() {
            self.emit_event(
                "content_block_stop",
                json!({ "type": "content_block_stop", "index": text_index }),
            );
        }

        // テキストも tool も無い空応答の場合は空テキストブロック(index 0)を保証する
        if self.next_block_index == 0 && self.tool_buffer.is_empty() {
            let text_index = self.ensure_text_block_open();
            self.open_text_index = None;
            self.emit_event(
                "content_block_stop",
                json!({ "type": "content_block_stop", "index": text_index }),
            );
        }

        // 蓄積した tool_use ブロックを単一オープンで逐次出力する（start→delta→stop）
        let buffered: Vec<StreamingToolCall> = std::mem::take(&mut self.tool_buffer)
            .into_values()
            .collect();
        for call in buffered {
            let block_index = self.next_block_index;
            self.next_block_index += 1;
            let tool_id = call
                .id
                .unwrap_or_else(|| format!("toolu_{}", Uuid::new_v4().simple()));
            let tool_name = call.name.unwrap_or_default();
            self.emit_event(
                "content_block_start",
                json!({
                    "type": "content_block_start",
                    "index": block_index,
                    "content_block": {
                        "type": "tool_use",
                        "id": tool_id,
                        "name": tool_name,
                        "input": {}
                    }
                }),
            );
            if !call.arguments.is_empty() {
                self.emit_event(
                    "content_block_delta",
                    json!({
                        "type": "content_block_delta",
                        "index": block_index,
                        "delta": {
                            "type": "input_json_delta",
                            "partial_json": call.arguments
                        }
                    }),
                );
            }
            self.emit_event(
                "content_block_stop",
                json!({ "type": "content_block_stop", "index": block_index }),
            );
            self.emitted_tool_use = true;
        }

        // tool_use を出力した場合は stop_reason を tool_use に補正する
        let effective_stop_reason =
            if self.emitted_tool_use && matches!(self.stop_reason, None | Some("end_turn")) {
                "tool_use"
            } else {
                self.stop_reason.unwrap_or("end_turn")
            };

        let usage = self.accumulator.finalize();
        self.emit_event(
            "message_delta",
            json!({
                "type": "message_delta",
                "delta": {
                    "stop_reason": effective_stop_reason,
                    "stop_sequence": self.stop_sequence
                },
                "usage": {
                    "output_tokens": usage.output_tokens.unwrap_or(0)
                }
            }),
        );
        self.emit_event("message_stop", json!({ "type": "message_stop" }));
        self.sent_message_stop = true;
    }

    fn emit_event(&mut self, event_name: &str, data: Value) {
        let payload = format!("event: {}\ndata: {}\n\n", event_name, data);
        self.output_queue.push_back(Bytes::from(payload));
    }

    fn record_stats_once(&mut self, success: bool, usage: TokenUsage) {
        if self.stats_recorded {
            return;
        }
        self.stats_recorded = true;

        let output_tokens = usage.output_tokens.unwrap_or(0) as u64;
        let duration_ms = if output_tokens > 0 {
            self.request_started_at.elapsed().as_millis().max(1) as u64
        } else {
            0
        };

        record_endpoint_request_stats(
            self.endpoint_registry.clone(),
            self.endpoint_id,
            self.model_id.clone(),
            success,
            output_tokens,
            duration_ms,
            Some(TpsApiKind::ChatCompletions),
            self.endpoint_type,
            self.load_manager.clone(),
            self.event_bus.clone(),
        );
    }
}

impl Drop for AnthropicStreamTracker {
    /// クライアント切断などで try_unfold が完走せず drop された場合のフォールバック。
    /// stats_recorded が false のまま破棄されると統計が漏れるため、ここで記録する。
    /// （proxy.rs の TpsTrackingState::drop を踏襲）
    fn drop(&mut self) {
        if self.stats_recorded {
            return;
        }

        // 未処理の行バッファをトークン集計へ反映してから確定する。
        if !self.upstream_line_buffer.is_empty() {
            let pending = std::mem::take(&mut self.upstream_line_buffer);
            self.accumulator
                .process_chunk(pending.trim_end_matches('\r'));
        }
        let usage = self.accumulator.finalize();

        // record_endpoint_request_stats は内部で tokio::spawn するため、ランタイム外で
        // drop されると panic しうる。Handle の存在を確認してから記録する。
        if tokio::runtime::Handle::try_current().is_ok() {
            self.record_stats_once(true, usage);
        } else {
            tracing::warn!(
                endpoint_id = %self.endpoint_id,
                model_id = %self.model_id,
                "Anthropic streaming tracker dropped without runtime; skipping stats fallback"
            );
        }
    }
}

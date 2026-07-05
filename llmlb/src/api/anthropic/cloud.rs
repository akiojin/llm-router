//! 実 Anthropic クラウド Messages API へのパススループロキシ
//!
//! arch-review [H6]: api/anthropic.rs から、anthropic: プレフィックス検出・
//! ヘッダ転送・ストリーミング/タイムアウト付き非ストリーミング・クラウドメトリクス
//! と履歴記録を行うクラウド転送ロジックを分離。親から pub(super) で参照される。

use super::response::build_response_from_upstream;
use super::{anthropic_error_response, UNSPECIFIED_IP};
use crate::api::error::AppError;
use crate::api::proxy::{forward_streaming_response, save_request_record};
use crate::cloud_metrics;
use crate::common::error::LbError;
use crate::common::protocol::{RecordStatus, RequestResponseRecord, RequestType};
use crate::token::extract_usage_from_response;
use crate::AppState;
use axum::http::StatusCode;
use axum::response::Response;
use serde_json::Value;
use std::net::IpAddr;
use std::time::Instant;
use uuid::Uuid;

const ANTHROPIC_CLOUD_ENDPOINT_ID: &str = "00000000-0000-0000-0000-00000000c003";

/// 非ストリーミングの Anthropic クラウド転送に適用する全体タイムアウト（秒）。
///
/// 共有 http_client には全体タイムアウトが無いため、応答しないクラウドで
/// 無期限ハングするのを防ぐ。ストリーミング経路にはストリームが途中で切れるため付与しない。
const ANTHROPIC_CLOUD_TIMEOUT_SECS: u64 = 300;

#[allow(clippy::too_many_arguments)]
pub(super) async fn proxy_anthropic_cloud_messages(
    state: &AppState,
    request_body: Value,
    public_model: String,
    cloud_model: String,
    anthropic_version: String,
    anthropic_beta: Option<String>,
    client_ip: Option<IpAddr>,
    api_key_id: Option<Uuid>,
) -> Result<Response, AppError> {
    let api_key = match std::env::var("ANTHROPIC_API_KEY") {
        Ok(value) => value,
        Err(_) => {
            return Ok(anthropic_error_response(
                StatusCode::SERVICE_UNAVAILABLE,
                "api_error",
                "Anthropic cloud integration is not configured",
            ));
        }
    };
    let base_url = std::env::var("ANTHROPIC_API_BASE_URL")
        .unwrap_or_else(|_| "https://api.anthropic.com".into());
    let url = format!("{}/v1/messages", base_url.trim_end_matches('/'));
    let stream = request_body
        .get("stream")
        .and_then(Value::as_bool)
        .unwrap_or(false);

    let endpoint_id = Uuid::parse_str(ANTHROPIC_CLOUD_ENDPOINT_ID)
        .expect("static anthropic cloud endpoint id must be valid");
    let mut upstream_body = request_body.clone();
    upstream_body["model"] = Value::String(cloud_model);

    let started = Instant::now();
    let mut builder = state
        .http_client
        .post(&url)
        .header("x-api-key", api_key)
        .header("anthropic-version", anthropic_version)
        .json(&upstream_body);
    if let Some(beta) = anthropic_beta {
        builder = builder.header("anthropic-beta", beta);
    }
    // 非ストリーミングは有界レスポンスのため全体タイムアウトを付与する。
    // ストリーミングは connect_timeout（共有 client）のみで担保し、
    // 全体タイムアウトでストリームが途中で切れるのを避ける。
    if !stream {
        builder = builder.timeout(std::time::Duration::from_secs(ANTHROPIC_CLOUD_TIMEOUT_SECS));
    }

    let upstream = match builder.send().await {
        Ok(response) => response,
        Err(err) => {
            let duration = started.elapsed();
            let mut record = RequestResponseRecord::new(
                endpoint_id,
                "cloud:anthropic".to_string(),
                UNSPECIFIED_IP,
                public_model,
                RequestType::AnthropicMessages,
                request_body,
                StatusCode::BAD_GATEWAY,
                duration,
                client_ip,
                api_key_id,
            );
            record.status = RecordStatus::Error {
                message: format!("Failed to proxy Anthropic cloud request: {}", err),
            };
            save_request_record(state.request_history.clone(), record);

            return Ok(anthropic_error_response(
                StatusCode::BAD_GATEWAY,
                "api_error",
                "Anthropic upstream request failed",
            ));
        }
    };

    let status =
        StatusCode::from_u16(upstream.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    cloud_metrics::record("anthropic", status.as_u16(), started.elapsed().as_millis());

    if stream && status.is_success() {
        let response = forward_streaming_response(upstream).map_err(AppError::from)?;
        let record = RequestResponseRecord::new(
            endpoint_id,
            "cloud:anthropic".to_string(),
            UNSPECIFIED_IP,
            public_model,
            RequestType::AnthropicMessages,
            request_body,
            status,
            started.elapsed(),
            client_ip,
            api_key_id,
        );
        save_request_record(state.request_history.clone(), record);
        return Ok(response);
    }

    let headers = upstream.headers().clone();
    let bytes = upstream.bytes().await.map_err(|err| {
        AppError::from(LbError::Http(format!(
            "Failed to read Anthropic cloud response body: {}",
            err
        )))
    })?;
    let parsed_body = serde_json::from_slice::<Value>(&bytes).ok();

    let mut record = RequestResponseRecord::new(
        endpoint_id,
        "cloud:anthropic".to_string(),
        UNSPECIFIED_IP,
        public_model,
        RequestType::AnthropicMessages,
        request_body,
        status,
        started.elapsed(),
        client_ip,
        api_key_id,
    );
    if let Some(body) = parsed_body.clone() {
        if status.is_success() {
            record.response_body = Some(body.clone());
            if let Some(usage) = extract_usage_from_response(&body) {
                record.input_tokens = usage.input_tokens;
                record.output_tokens = usage.output_tokens;
                record.total_tokens = usage.total_tokens;
            }
        } else {
            record.status = RecordStatus::Error {
                message: body
                    .get("error")
                    .and_then(|v| v.get("message"))
                    .and_then(Value::as_str)
                    .unwrap_or_else(|| status.as_str())
                    .to_string(),
            };
        }
    } else if !status.is_success() {
        record.status = RecordStatus::Error {
            message: String::from_utf8_lossy(&bytes).trim().to_string(),
        };
    }
    save_request_record(state.request_history.clone(), record);

    Ok(build_response_from_upstream(status, &headers, bytes))
}

pub(super) fn parse_anthropic_cloud_model(model: &str) -> Option<String> {
    if let Some(stripped) = model.strip_prefix("anthropic:") {
        if !stripped.is_empty() {
            return Some(stripped.to_string());
        }
    }
    if let Some(stripped) = model.strip_prefix("ahtnorpic:") {
        if !stripped.is_empty() {
            return Some(stripped.to_string());
        }
    }
    None
}

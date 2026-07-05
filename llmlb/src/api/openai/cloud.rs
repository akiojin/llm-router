//! OpenAI 互換APIのクラウドプロバイダー転送。
//!
//! `provider:model` 形式のモデル指定を解釈し、対応するクラウドプロバイダーへ
//! リクエストを転送する。仮想ノード ID の生成とリクエスト履歴記録を担う。

use super::{validation_error, UNSPECIFIED_IP};
use crate::api::cloud_proxy::{proxy_cloud_provider, resolve_provider};
use crate::api::error::AppError;
use crate::api::openai_util::sanitize_openai_payload_for_history;
use crate::api::proxy::save_request_record;
use crate::common::protocol::{RecordStatus, RequestResponseRecord, RequestType};
use crate::AppState;
use axum::http::StatusCode;
use axum::response::Response;
use serde_json::Value;
use std::net::IpAddr;
use std::time::Instant;
use uuid::Uuid;

pub(super) fn parse_cloud_model(model: &str) -> Option<(String, String)> {
    // Accept prefixes like "openai:foo", "google:bar", "anthropic:baz"
    let prefixes = ["openai:", "google:", "anthropic:", "ahtnorpic:"];
    for p in prefixes.iter() {
        if model.starts_with(p) {
            let rest = model.trim_start_matches(p);
            if rest.is_empty() {
                return None;
            }
            let provider = if *p == "ahtnorpic:" {
                "anthropic"
            } else {
                p.trim_end_matches(':')
            };
            return Some((provider.to_string(), rest.to_string()));
        }
    }
    None
}

/// クラウドプロバイダ用の仮想ノード情報を生成する
pub(super) fn cloud_virtual_node(provider: &str) -> (Uuid, String, IpAddr) {
    // 仮想ノードIDはクラウドプロバイダごとに固定値
    let endpoint_id = match provider {
        "openai" => Uuid::parse_str("00000000-0000-0000-0000-00000000c001")
            .expect("static UUID string is valid"),
        "google" => Uuid::parse_str("00000000-0000-0000-0000-00000000c002")
            .expect("static UUID string is valid"),
        "anthropic" => Uuid::parse_str("00000000-0000-0000-0000-00000000c003")
            .expect("static UUID string is valid"),
        _ => Uuid::parse_str("00000000-0000-0000-0000-00000000c0ff")
            .expect("static UUID string is valid"),
    };
    let machine_name = format!("cloud:{provider}");
    (endpoint_id, machine_name, UNSPECIFIED_IP)
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn proxy_openai_cloud_post(
    state: &AppState,
    _target_path: &str,
    model: &str,
    stream: bool,
    payload: Value,
    request_type: RequestType,
    client_ip: Option<IpAddr>,
    api_key_id: Option<Uuid>,
) -> Result<Response, AppError> {
    let (provider, model_name) = parse_cloud_model(model)
        .ok_or_else(|| validation_error("cloud model prefix is invalid"))?;
    let (endpoint_id, endpoint_name, endpoint_ip) = cloud_virtual_node(&provider);
    let request_body = sanitize_openai_payload_for_history(&payload);
    let started = Instant::now();

    let cloud_provider = resolve_provider(provider.as_str())
        .ok_or_else(|| validation_error("unsupported cloud provider prefix"))?;
    let outcome = match proxy_cloud_provider(
        cloud_provider.as_ref(),
        &state.http_client,
        &payload,
        &model_name,
        stream,
    )
    .await
    {
        Ok(res) => res,
        Err(e) => {
            let duration = started.elapsed();
            {
                let mut record = RequestResponseRecord::new(
                    endpoint_id,
                    endpoint_name,
                    endpoint_ip,
                    model.to_string(),
                    request_type,
                    request_body,
                    StatusCode::BAD_GATEWAY,
                    duration,
                    client_ip,
                    api_key_id,
                );
                record.status = RecordStatus::Error {
                    message: format!("{e:?}"),
                };
                save_request_record(state.request_history.clone(), record);
            }
            return Err(e);
        }
    };

    let duration = started.elapsed();
    let status = outcome.status;
    {
        let mut record = RequestResponseRecord::new(
            endpoint_id,
            endpoint_name,
            endpoint_ip,
            model.to_string(),
            request_type,
            request_body,
            status,
            duration,
            client_ip,
            api_key_id,
        );
        if !status.is_success() {
            record.status = RecordStatus::Error {
                message: outcome
                    .error_message
                    .clone()
                    .unwrap_or_else(|| status.to_string()),
            };
        }
        if status.is_success() {
            record.response_body = outcome.response_body.clone();
        }
        save_request_record(state.request_history.clone(), record);
    }

    Ok(outcome.response)
}

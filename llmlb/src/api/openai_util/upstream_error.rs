//! 上流（reqwest）リクエスト失敗の分類とレスポンスボディの上限付き読み取り
//!
//! arch-review [H6]: api/openai_util.rs から上流エラー分類ロジックを分離。

use axum::http::StatusCode;
use std::error::Error as StdError;

/// Structured classification for an upstream request failure.
pub struct UpstreamRequestFailure {
    /// HTTP status returned to the client.
    pub status_code: StatusCode,
    /// Stable error type string used by the OpenAI-compatible error body.
    pub error_type: &'static str,
    /// User-facing error message.
    pub client_message: String,
    /// Detailed message stored in request history.
    pub record_message: String,
}

/// Classifies a reqwest upstream failure into a stable client-facing error response.
pub fn classify_upstream_request_error(
    error: &reqwest::Error,
    upstream_url: &str,
    timeout_secs: u32,
    ollama_loading_model: Option<&str>,
) -> UpstreamRequestFailure {
    if error.is_timeout() {
        if let Some(model) = ollama_loading_model {
            let client_message = format!(
                "Ollama model '{}' is still loading. Retry after the initial load finishes or increase endpoint inference timeout above {} seconds.",
                model, timeout_secs
            );
            return UpstreamRequestFailure {
                status_code: StatusCode::GATEWAY_TIMEOUT,
                error_type: "model_loading",
                record_message: format!("{}: {}", client_message, error),
                client_message,
            };
        }

        let client_message =
            format!("Upstream request to {upstream_url} timed out after {timeout_secs} seconds");
        return UpstreamRequestFailure {
            status_code: StatusCode::GATEWAY_TIMEOUT,
            error_type: "timeout",
            record_message: format!("{}: {}", client_message, error),
            client_message,
        };
    }

    if error.is_connect() {
        let client_message = format!("Failed to connect to upstream: {upstream_url}");
        return UpstreamRequestFailure {
            status_code: StatusCode::BAD_GATEWAY,
            error_type: "connection_error",
            record_message: format!("{}: {}", client_message, error),
            client_message,
        };
    }

    if is_tls_error(error) {
        let client_message = format!("Upstream TLS handshake failed: {upstream_url}");
        return UpstreamRequestFailure {
            status_code: StatusCode::BAD_GATEWAY,
            error_type: "tls_error",
            record_message: format!("{}: {}", client_message, error),
            client_message,
        };
    }

    let client_message = format!("Upstream request failed: {upstream_url}");
    UpstreamRequestFailure {
        status_code: StatusCode::BAD_GATEWAY,
        error_type: "endpoint_request_error",
        record_message: format!("{}: {}", client_message, error),
        client_message,
    }
}

/// Build a client-visible upstream error message from an HTTP status and raw body bytes.
pub fn upstream_error_message_from_bytes(status: StatusCode, body_bytes: &[u8]) -> String {
    let body = String::from_utf8_lossy(body_bytes).trim().to_string();
    if body.is_empty() {
        status.to_string()
    } else {
        body
    }
}

/// 上流エラー要約のために読み取るボディの最大バイト数（4KB）。
///
/// エラーメッセージの要約には先頭の数 KB で十分であり、悪意/誤動作の上流が
/// 巨大なエラーボディを返してもメモリを枯渇させないための上限。
pub const UPSTREAM_ERROR_SUMMARY_MAX_BYTES: usize = 4096;

/// レスポンスボディを先頭 `max_bytes` までに制限して読み取る。
///
/// `response.bytes()` は全文をメモリに展開するため、エラー要約のように
/// 先頭部分しか必要としない用途では本関数で読み取り量を上限化する。
/// 残りのチャンクは読まずに破棄する。読み取り中のエラーはそこまでの
/// 内容を返す（要約用途では致命的でない）。
pub async fn read_capped_body(response: reqwest::Response, max_bytes: usize) -> Vec<u8> {
    use futures::StreamExt;

    let mut stream = response.bytes_stream();
    let mut buf: Vec<u8> = Vec::new();
    while buf.len() < max_bytes {
        match stream.next().await {
            Some(Ok(chunk)) => {
                let remaining = max_bytes - buf.len();
                if chunk.len() > remaining {
                    buf.extend_from_slice(&chunk[..remaining]);
                    break;
                }
                buf.extend_from_slice(&chunk);
            }
            Some(Err(_)) | None => break,
        }
    }
    buf
}

fn is_tls_error(error: &reqwest::Error) -> bool {
    error_chain_contains(error, &["certificate", "cert", "tls", "ssl", "handshake"])
}

fn error_chain_contains(error: &reqwest::Error, needles: &[&str]) -> bool {
    let mut current = error.source();
    while let Some(source) = current {
        let text = source.to_string().to_ascii_lowercase();
        if needles.iter().any(|needle| text.contains(needle)) {
            return true;
        }
        current = source.source();
    }
    false
}

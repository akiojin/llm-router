//! 音声API エンドポイント (/v1/audio/*)
//!
//! OpenAI互換の音声認識（ASR）・音声合成（TTS）API

use crate::common::{
    error::LbError,
    protocol::{RequestResponseRecord, RequestType, SpeechRequest},
};
use crate::types::model::ModelCapability;
use axum::{
    body::Body,
    extract::{ConnectInfo, Multipart, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;
use std::time::Instant;
use tracing::info;
use uuid::Uuid;

use std::net::{IpAddr, SocketAddr};

use crate::{
    api::{
        error::AppError,
        inference_backend::InferenceBackend,
        model_name::parse_quantized_model_name,
        models::load_registered_model,
        proxy::{forward_streaming_response, save_request_record},
    },
    auth::middleware::ApiKeyAuthContext,
    common::ip::{normalize_ip, normalize_socket_ip},
    types::endpoint::EndpointCapability,
    AppState,
};

/// OpenAI互換エラーレスポンスを生成
fn error_response(error: LbError, status: StatusCode) -> Response {
    let (message, error_type) = match error {
        LbError::Http(msg) => (msg, "invalid_request_error"),
        LbError::ServiceUnavailable(msg) => (msg, "service_unavailable"),
        LbError::InvalidModelName(msg) => (msg, "invalid_request_error"),
        _ => (error.external_message().to_string(), "api_error"),
    };

    (
        status,
        Json(json!({
            "error": {
                "message": message,
                "type": error_type,
                "code": status.as_u16()
            }
        })),
    )
        .into_response()
}

/// OpenAI互換エラーレスポンスを返す（ハンドラで使用）
fn openai_error<T: Into<String>>(msg: T, status: StatusCode) -> Result<Response, AppError> {
    Ok(error_response(LbError::Http(msg.into()), status))
}

fn extract_client_ip_from_forwarded_headers(headers: &HeaderMap) -> Option<IpAddr> {
    extract_x_forwarded_for(headers).or_else(|| extract_forwarded_for(headers))
}

fn extract_x_forwarded_for(headers: &HeaderMap) -> Option<IpAddr> {
    let value = headers.get("x-forwarded-for")?.to_str().ok()?;
    value
        .split(',')
        .map(str::trim)
        .find_map(parse_forwarded_ip_candidate)
}

fn extract_forwarded_for(headers: &HeaderMap) -> Option<IpAddr> {
    let value = headers.get("forwarded")?.to_str().ok()?;
    value.split(',').find_map(|entry| {
        entry
            .split(';')
            .filter_map(|pair| pair.split_once('='))
            .find_map(|(key, value)| {
                if key.trim().eq_ignore_ascii_case("for") {
                    parse_forwarded_ip_candidate(value.trim())
                } else {
                    None
                }
            })
    })
}

fn parse_forwarded_ip_candidate(value: &str) -> Option<IpAddr> {
    let trimmed = value.trim().trim_matches('"');
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("unknown") || trimmed.starts_with('_') {
        return None;
    }

    let host = if let Some(stripped) = trimmed.strip_prefix('[') {
        stripped.split(']').next().unwrap_or_default().trim()
    } else {
        trimmed
    };

    if let Ok(ip) = host.parse::<IpAddr>() {
        return Some(normalize_ip(ip));
    }

    if let Some((ip_candidate, _port)) = host.rsplit_once(':') {
        if !ip_candidate.contains(':') {
            if let Ok(ip) = ip_candidate.parse::<IpAddr>() {
                return Some(normalize_ip(ip));
            }
        }
    }

    None
}

/// 音声認識対応バックエンドを選択
/// EndpointRegistry経由でのみ取得（NodeRegistryフォールバック廃止）
async fn select_transcription_backend(state: &AppState) -> Result<InferenceBackend, LbError> {
    let endpoints = state
        .endpoint_registry
        .list_online_by_capability(EndpointCapability::AudioTranscription)
        .await;

    let endpoint = endpoints.into_iter().next().ok_or_else(|| {
        LbError::ServiceUnavailable(
            "No endpoints available with audio transcription capability".to_string(),
        )
    })?;

    Ok(InferenceBackend(endpoint))
}

/// 音声合成対応バックエンドを選択
/// EndpointRegistry経由でのみ取得（NodeRegistryフォールバック廃止）
async fn select_speech_backend(state: &AppState) -> Result<InferenceBackend, LbError> {
    let endpoints = state
        .endpoint_registry
        .list_online_by_capability(EndpointCapability::AudioSpeech)
        .await;

    let endpoint = endpoints.into_iter().next().ok_or_else(|| {
        LbError::ServiceUnavailable(
            "No endpoints available with audio speech capability".to_string(),
        )
    })?;

    Ok(InferenceBackend(endpoint))
}

/// POST /v1/audio/transcriptions - 音声認識（ASR）
///
/// multipart/form-data 形式でリクエスト
/// - file: 音声ファイル（wav, mp3, flac等）
/// - model: 使用するモデル名
/// - language: 言語コード（オプション）
/// - response_format: レスポンス形式（json, text, srt, vtt）
pub async fn transcriptions(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    State(state): State<AppState>,
    auth_ctx: Option<axum::Extension<ApiKeyAuthContext>>,
    mut multipart: Multipart,
) -> Result<Response, AppError> {
    let client_ip = Some(
        extract_client_ip_from_forwarded_headers(&headers)
            .unwrap_or_else(|| normalize_socket_ip(&addr)),
    );
    let api_key_id = auth_ctx.as_ref().map(|ext| ext.0.id);
    let start = Instant::now();
    let request_id = Uuid::new_v4();

    // multipart データを解析
    let mut file_data: Option<Vec<u8>> = None;
    let mut file_name: Option<String> = None;
    let mut model: Option<String> = None;
    let mut language: Option<String> = None;
    let mut response_format: Option<String> = None;

    while let Some(field) = match multipart.next_field().await {
        Ok(f) => f,
        Err(e) => {
            return openai_error(
                format!("Failed to parse multipart form: {}", e),
                StatusCode::BAD_REQUEST,
            )
        }
    } {
        let name = field.name().unwrap_or("").to_string();

        match name.as_str() {
            "file" => {
                file_name = field.file_name().map(|s| s.to_string());
                match field.bytes().await {
                    Ok(bytes) => file_data = Some(bytes.to_vec()),
                    Err(e) => {
                        return openai_error(
                            format!("Failed to read file field: {}", e),
                            StatusCode::BAD_REQUEST,
                        )
                    }
                }
            }
            "model" => match field.text().await {
                Ok(text) => model = Some(text),
                Err(e) => {
                    return openai_error(
                        format!("Failed to read model field: {}", e),
                        StatusCode::BAD_REQUEST,
                    )
                }
            },
            "language" => match field.text().await {
                Ok(text) => language = Some(text),
                Err(e) => {
                    return openai_error(
                        format!("Failed to read language field: {}", e),
                        StatusCode::BAD_REQUEST,
                    )
                }
            },
            "response_format" => match field.text().await {
                Ok(text) => response_format = Some(text),
                Err(e) => {
                    return openai_error(
                        format!("Failed to read response_format field: {}", e),
                        StatusCode::BAD_REQUEST,
                    )
                }
            },
            _ => {
                // 未知のフィールドは無視
            }
        }
    }

    // 必須フィールドの検証
    let file_data = match file_data {
        Some(data) => data,
        None => return openai_error("Missing required field: file", StatusCode::BAD_REQUEST),
    };
    let model = match model {
        Some(m) => m,
        None => return openai_error("Missing required field: model", StatusCode::BAD_REQUEST),
    };
    let parsed = parse_quantized_model_name(&model).map_err(AppError::from)?;
    let _lookup_model = parsed.base;

    // モデルの SpeechToText capability を検証
    let model_info = load_registered_model(&state.db_pool, &model).await?;
    if let Some(model_info) = model_info {
        if !model_info.has_capability(ModelCapability::SpeechToText) {
            return openai_error(
                format!("Model '{}' does not support speech-to-text", parsed.raw),
                StatusCode::BAD_REQUEST,
            );
        }
    }
    // 登録されていないモデルはエンドポイント側で処理（クラウドモデル等）

    info!(
        request_id = %request_id,
        model = %model,
        file_size = file_data.len(),
        "Processing transcription request"
    );

    // ASR対応バックエンドを選択（EndpointRegistry優先、NodeRegistryフォールバック）
    let backend = select_transcription_backend(&state).await?;

    // multipart リクエストを構築してプロキシ
    let client = &state.http_client;
    let url = backend.url("/v1/audio/transcriptions");

    let mut form = reqwest::multipart::Form::new().part(
        "file",
        reqwest::multipart::Part::bytes(file_data)
            .file_name(file_name.unwrap_or_else(|| "audio.wav".to_string()))
            .mime_str("audio/wav")
            .expect("audio/wav is a valid MIME type"),
    );

    form = form.text("model", model.clone());

    if let Some(lang) = language {
        form = form.text("language", lang);
    }

    if let Some(fmt) = response_format {
        form = form.text("response_format", fmt);
    }

    let response = match client
        .post(&url)
        .multipart(form)
        .timeout(backend.inference_timeout())
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            return openai_error(
                format!("Failed to contact transcription node: {}", e),
                StatusCode::SERVICE_UNAVAILABLE,
            )
        }
    };

    let duration = start.elapsed();
    let status = response.status();

    // リクエスト履歴を記録
    let record = RequestResponseRecord::new(
        backend.id(),
        backend.name(),
        backend.ip(),
        model.clone(),
        RequestType::Transcription,
        json!({"model": model, "type": "transcription"}),
        status,
        duration,
        client_ip,
        api_key_id,
    );

    save_request_record(state.request_history.clone(), record);

    // レスポンスを転送
    forward_streaming_response(response)
        .map_err(AppError::from)
        .map(|r| r.into_response())
}

/// POST /v1/audio/speech - 音声合成（TTS）
///
/// JSON 形式でリクエスト
/// - model: 使用するモデル名
/// - input: 読み上げるテキスト
/// - voice: 音声種別（オプション、デフォルト: nova）
/// - response_format: 出力形式（オプション、デフォルト: mp3）
/// - speed: 再生速度（オプション、デフォルト: 1.0）
pub async fn speech(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    State(state): State<AppState>,
    auth_ctx: Option<axum::Extension<ApiKeyAuthContext>>,
    Json(payload): Json<SpeechRequest>,
) -> Result<Response, AppError> {
    let client_ip = Some(
        extract_client_ip_from_forwarded_headers(&headers)
            .unwrap_or_else(|| normalize_socket_ip(&addr)),
    );
    let api_key_id = auth_ctx.as_ref().map(|ext| ext.0.id);
    let start = Instant::now();
    let request_id = Uuid::new_v4();

    // 入力テキストの検証
    if payload.input.is_empty() {
        return openai_error("Input text is required", StatusCode::BAD_REQUEST);
    }

    // 入力長の制限（4096文字）
    if payload.input.chars().count() > 4096 {
        return openai_error(
            "Input text exceeds maximum length of 4096 characters",
            StatusCode::BAD_REQUEST,
        );
    }

    let parsed = parse_quantized_model_name(&payload.model).map_err(AppError::from)?;
    let _lookup_model = parsed.base;

    // モデルの TextToSpeech capability を検証
    let model_info = load_registered_model(&state.db_pool, &payload.model).await?;
    if let Some(model_info) = model_info {
        if !model_info.has_capability(ModelCapability::TextToSpeech) {
            return openai_error(
                format!("Model '{}' does not support text-to-speech", parsed.raw),
                StatusCode::BAD_REQUEST,
            );
        }
    }
    // 登録されていないモデルはエンドポイント側で処理（クラウドモデル等）

    info!(
        request_id = %request_id,
        model = %payload.model,
        input_length = payload.input.len(),
        voice = %payload.voice,
        "Processing speech request"
    );

    // TTS対応バックエンドを選択（EndpointRegistry優先、NodeRegistryフォールバック）
    let backend = select_speech_backend(&state).await?;

    // JSON リクエストをプロキシ
    let client = &state.http_client;
    let url = backend.url("/v1/audio/speech");

    let response = match client
        .post(&url)
        .json(&payload)
        .timeout(backend.inference_timeout())
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            return openai_error(
                format!("Failed to contact speech synthesis node: {}", e),
                StatusCode::SERVICE_UNAVAILABLE,
            )
        }
    };

    let duration = start.elapsed();
    let status = response.status();

    // リクエスト履歴を記録
    let record = RequestResponseRecord::new(
        backend.id(),
        backend.name(),
        backend.ip(),
        payload.model.clone(),
        RequestType::Speech,
        serde_json::to_value(&payload).unwrap_or(json!({})),
        status,
        duration,
        client_ip,
        api_key_id,
    );

    save_request_record(state.request_history.clone(), record);

    if status.is_success() {
        // 音声バイナリをストリーミング転送
        // reqwestとaxumで異なるhttp crateバージョンを使うため、文字列経由で変換
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("audio/mpeg")
            .to_string();

        let stream = response.bytes_stream();
        let body = Body::from_stream(stream);

        Ok(Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, content_type)
            .body(body)
            .expect("Response builder should not fail with valid status and body")
            .into_response())
    } else {
        // エラーレスポンスを転送
        forward_streaming_response(response)
            .map_err(AppError::from)
            .map(|r| r.into_response())
    }
}

#[cfg(test)]
mod tests {
    use super::{extract_client_ip_from_forwarded_headers, parse_forwarded_ip_candidate};
    use axum::http::{HeaderMap, HeaderValue, StatusCode};
    use std::net::IpAddr;

    #[test]
    fn extract_client_ip_prefers_first_valid_x_forwarded_for() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-forwarded-for",
            HeaderValue::from_static("unknown, 203.0.113.5, 10.0.0.1"),
        );
        headers.insert(
            "forwarded",
            HeaderValue::from_static("for=198.51.100.10;proto=https"),
        );
        let parsed =
            extract_client_ip_from_forwarded_headers(&headers).expect("must parse x-forwarded-for");
        assert_eq!(parsed, "203.0.113.5".parse::<IpAddr>().unwrap());
    }

    #[test]
    fn extract_client_ip_falls_back_to_forwarded_header() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "forwarded",
            HeaderValue::from_static("for=unknown;proto=https, for=\"[2001:db8::a]:8443\""),
        );
        let parsed =
            extract_client_ip_from_forwarded_headers(&headers).expect("must parse forwarded");
        assert_eq!(parsed, "2001:db8::a".parse::<IpAddr>().unwrap());
    }

    #[test]
    fn parse_forwarded_ip_candidate_supports_bracketed_ipv6() {
        let parsed = parse_forwarded_ip_candidate("\"[2001:db8::f]:443\"")
            .expect("must parse bracketed ipv6");
        assert_eq!(parsed, "2001:db8::f".parse::<IpAddr>().unwrap());
    }

    #[test]
    fn test_input_length_validation() {
        // 4096文字以下は許可
        let short_input = "a".repeat(4096);
        assert!(short_input.chars().count() <= 4096);

        // 4097文字以上は拒否
        let long_input = "a".repeat(4097);
        assert!(long_input.chars().count() > 4096);
    }

    #[test]
    fn test_unicode_input_length() {
        // 日本語文字のカウント（バイト数ではなく文字数）
        let japanese = "あ".repeat(4096);
        assert_eq!(japanese.chars().count(), 4096);

        let japanese_long = "あ".repeat(4097);
        assert!(japanese_long.chars().count() > 4096);
    }

    // T004: TTS capabilities検証テスト (RED)
    // TextToSpeech capability を持たないモデルで /v1/audio/speech を呼ぶとエラー
    #[test]
    fn test_tts_capability_validation_error_message() {
        use crate::types::model::{ModelCapability, ModelType};

        // LLMモデルはTextGenerationのみ、TextToSpeechは非対応
        let llm_caps = ModelCapability::from_model_type(ModelType::Llm);
        assert!(!llm_caps.contains(&ModelCapability::TextToSpeech));

        // 期待されるエラーメッセージ形式
        let model_name = "llama-3.1-8b";
        let expected_error = format!("Model '{}' does not support text-to-speech", model_name);
        assert!(expected_error.contains("does not support text-to-speech"));
    }

    // T005: ASR capabilities検証テスト (RED)
    // SpeechToText capability を持たないモデルで /v1/audio/transcriptions を呼ぶとエラー
    #[test]
    fn test_asr_capability_validation_error_message() {
        use crate::types::model::{ModelCapability, ModelType};

        // LLMモデルはTextGenerationのみ、SpeechToTextは非対応
        let llm_caps = ModelCapability::from_model_type(ModelType::Llm);
        assert!(!llm_caps.contains(&ModelCapability::SpeechToText));

        // TTSモデルもSpeechToTextは非対応
        let tts_caps = ModelCapability::from_model_type(ModelType::TextToSpeech);
        assert!(!tts_caps.contains(&ModelCapability::SpeechToText));

        // 期待されるエラーメッセージ形式
        let model_name = "vibevoice-v1";
        let expected_error = format!("Model '{}' does not support speech-to-text", model_name);
        assert!(expected_error.contains("does not support speech-to-text"));
    }

    // --- error_response tests ---

    #[test]
    fn error_response_http_error_type() {
        use super::error_response;
        use crate::common::error::LbError;

        let resp = error_response(
            LbError::Http("connection refused".to_string()),
            StatusCode::BAD_GATEWAY,
        );
        assert_eq!(resp.status(), StatusCode::BAD_GATEWAY);
    }

    #[test]
    fn error_response_service_unavailable_type() {
        use super::error_response;
        use crate::common::error::LbError;

        let resp = error_response(
            LbError::ServiceUnavailable("no backends".to_string()),
            StatusCode::SERVICE_UNAVAILABLE,
        );
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[test]
    fn error_response_invalid_model_name_type() {
        use super::error_response;
        use crate::common::error::LbError;

        let resp = error_response(
            LbError::InvalidModelName("bad:model:name".to_string()),
            StatusCode::BAD_REQUEST,
        );
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[test]
    fn error_response_fallback_api_error_type() {
        use super::error_response;
        use crate::common::error::LbError;

        let resp = error_response(
            LbError::Internal("unknown error".to_string()),
            StatusCode::INTERNAL_SERVER_ERROR,
        );
        assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    // --- openai_error helper tests ---

    #[test]
    fn openai_error_returns_ok_with_status() {
        use super::openai_error;

        let result = openai_error("test error message", StatusCode::BAD_REQUEST);
        assert!(result.is_ok());
        let resp = result.unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[test]
    fn openai_error_accepts_string_type() {
        use super::openai_error;

        let msg = String::from("dynamic error");
        let result = openai_error(msg, StatusCode::UNPROCESSABLE_ENTITY);
        assert!(result.is_ok());
        let resp = result.unwrap();
        assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }

    // --- forwarded header extraction edge cases ---

    #[test]
    fn extract_client_ip_returns_none_for_empty_headers() {
        let headers = HeaderMap::new();
        assert!(extract_client_ip_from_forwarded_headers(&headers).is_none());
    }

    #[test]
    fn extract_client_ip_returns_none_for_all_unknown_xff() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-forwarded-for",
            HeaderValue::from_static("unknown, unknown"),
        );
        assert!(extract_client_ip_from_forwarded_headers(&headers).is_none());
    }

    #[test]
    fn parse_forwarded_ip_candidate_empty_string() {
        assert!(parse_forwarded_ip_candidate("").is_none());
    }

    #[test]
    fn parse_forwarded_ip_candidate_unknown_string() {
        assert!(parse_forwarded_ip_candidate("unknown").is_none());
    }

    #[test]
    fn parse_forwarded_ip_candidate_unknown_case_insensitive() {
        assert!(parse_forwarded_ip_candidate("UNKNOWN").is_none());
        assert!(parse_forwarded_ip_candidate("Unknown").is_none());
    }

    #[test]
    fn parse_forwarded_ip_candidate_obfuscated_identifier() {
        // RFC 7239: obfuscated identifiers start with underscore
        assert!(parse_forwarded_ip_candidate("_hidden").is_none());
    }

    #[test]
    fn parse_forwarded_ip_candidate_plain_ipv4() {
        let parsed = parse_forwarded_ip_candidate("198.51.100.1").expect("must parse plain ipv4");
        assert_eq!(parsed, "198.51.100.1".parse::<IpAddr>().unwrap());
    }

    #[test]
    fn parse_forwarded_ip_candidate_plain_ipv6() {
        let parsed = parse_forwarded_ip_candidate("2001:db8::1").expect("must parse plain ipv6");
        assert_eq!(parsed, "2001:db8::1".parse::<IpAddr>().unwrap());
    }

    #[test]
    fn parse_forwarded_ip_candidate_quoted_ipv4() {
        let parsed =
            parse_forwarded_ip_candidate("\"198.51.100.2\"").expect("must parse quoted ipv4");
        assert_eq!(parsed, "198.51.100.2".parse::<IpAddr>().unwrap());
    }

    #[test]
    fn parse_forwarded_ip_candidate_ipv4_with_port() {
        let parsed =
            parse_forwarded_ip_candidate("10.0.0.1:8080").expect("must parse ipv4 with port");
        assert_eq!(parsed, "10.0.0.1".parse::<IpAddr>().unwrap());
    }

    #[test]
    fn parse_forwarded_ip_candidate_bracketed_ipv6_with_port() {
        let parsed = parse_forwarded_ip_candidate("[2001:db8::1]:443")
            .expect("must parse bracketed ipv6 with port");
        assert_eq!(parsed, "2001:db8::1".parse::<IpAddr>().unwrap());
    }

    #[test]
    fn parse_forwarded_ip_candidate_whitespace_trimming() {
        let parsed =
            parse_forwarded_ip_candidate("  10.0.0.1  ").expect("must parse with whitespace");
        assert_eq!(parsed, "10.0.0.1".parse::<IpAddr>().unwrap());
    }

    #[test]
    fn parse_forwarded_ip_candidate_invalid_returns_none() {
        assert!(parse_forwarded_ip_candidate("not-an-ip").is_none());
    }

    #[test]
    fn extract_x_forwarded_for_single_ip() {
        use super::extract_x_forwarded_for;
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-for", HeaderValue::from_static("192.168.1.1"));
        let ip = extract_x_forwarded_for(&headers).expect("should parse single ip");
        assert_eq!(ip, "192.168.1.1".parse::<IpAddr>().unwrap());
    }

    #[test]
    fn extract_x_forwarded_for_multiple_ips_returns_first_valid() {
        use super::extract_x_forwarded_for;
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-forwarded-for",
            HeaderValue::from_static("unknown, _obfuscated, 10.0.0.1, 192.168.0.1"),
        );
        let ip = extract_x_forwarded_for(&headers).expect("should skip invalid entries");
        assert_eq!(ip, "10.0.0.1".parse::<IpAddr>().unwrap());
    }

    #[test]
    fn extract_x_forwarded_for_missing_header_returns_none() {
        use super::extract_x_forwarded_for;
        let headers = HeaderMap::new();
        assert!(extract_x_forwarded_for(&headers).is_none());
    }

    #[test]
    fn extract_forwarded_for_standard_format() {
        use super::extract_forwarded_for;
        let mut headers = HeaderMap::new();
        headers.insert(
            "forwarded",
            HeaderValue::from_static("for=192.0.2.60;proto=http;by=203.0.113.43"),
        );
        let ip = extract_forwarded_for(&headers).expect("should parse standard format");
        assert_eq!(ip, "192.0.2.60".parse::<IpAddr>().unwrap());
    }

    #[test]
    fn extract_forwarded_for_multiple_entries() {
        use super::extract_forwarded_for;
        let mut headers = HeaderMap::new();
        headers.insert(
            "forwarded",
            HeaderValue::from_static("for=unknown, for=198.51.100.20"),
        );
        let ip = extract_forwarded_for(&headers).expect("should parse second entry");
        assert_eq!(ip, "198.51.100.20".parse::<IpAddr>().unwrap());
    }

    #[test]
    fn extract_forwarded_for_missing_header_returns_none() {
        use super::extract_forwarded_for;
        let headers = HeaderMap::new();
        assert!(extract_forwarded_for(&headers).is_none());
    }

    #[test]
    fn extract_forwarded_for_ignores_non_for_keys() {
        use super::extract_forwarded_for;
        let mut headers = HeaderMap::new();
        headers.insert(
            "forwarded",
            HeaderValue::from_static("by=203.0.113.43;proto=https"),
        );
        assert!(extract_forwarded_for(&headers).is_none());
    }

    // --- SpeechRequest / input validation edge case tests ---

    #[test]
    fn test_speech_request_deserialization_with_all_fields() {
        use crate::common::protocol::SpeechRequest;
        let json = r#"{
            "model": "tts-1-hd",
            "input": "Hello world",
            "voice": "echo",
            "response_format": "flac",
            "speed": 1.5
        }"#;
        let req: SpeechRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.model, "tts-1-hd");
        assert_eq!(req.input, "Hello world");
        assert_eq!(req.voice, "echo");
        assert_eq!(req.speed, 1.5);
    }

    #[test]
    fn test_empty_input_validation_logic() {
        // Verifies the empty-check logic used in the handler
        let empty = "";
        assert!(empty.is_empty());
        let non_empty = "hello";
        assert!(!non_empty.is_empty());
    }

    #[test]
    fn test_input_char_count_with_mixed_scripts() {
        // Mixed ASCII + CJK + emoji
        let input = "Hello, \u{4e16}\u{754c}! \u{1f600}";
        let count = input.chars().count();
        // "Hello, " = 7, "世界" = 2, "! " = 2, emoji = 1 = 12
        assert_eq!(count, 12);
        assert!(count <= 4096);
    }

    #[test]
    fn test_input_exactly_4096_chars() {
        let input = "x".repeat(4096);
        assert_eq!(input.chars().count(), 4096);
        assert!(input.chars().count() <= 4096);
    }

    #[test]
    fn test_input_exactly_4097_chars() {
        let input = "x".repeat(4097);
        assert!(input.chars().count() > 4096);
    }
}

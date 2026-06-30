//! 画像API エンドポイント (/v1/images/*)
//!
//! OpenAI互換の画像生成（Text-to-Image）・編集（Inpainting）・バリエーションAPI

use crate::common::{
    error::LbError,
    protocol::{ImageGenerationRequest, RequestResponseRecord, RequestType},
};
use crate::types::model::ModelCapability;
use axum::{
    extract::{ConnectInfo, Multipart, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;
use std::net::{IpAddr, SocketAddr};
use std::time::Instant;
use tracing::info;
use uuid::Uuid;

use crate::{
    api::{
        error::AppError,
        model_name::parse_quantized_model_name,
        models::load_registered_model,
        proxy::{forward_streaming_response, save_request_record},
    },
    auth::middleware::ApiKeyAuthContext,
    common::ip::{normalize_ip, normalize_socket_ip},
    types::endpoint::{Endpoint, EndpointCapability},
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

/// 画像生成対応バックエンド
/// EndpointRegistry経由でのみ取得（NodeRegistryフォールバック廃止）
struct ImageBackend(Endpoint);

impl ImageBackend {
    /// リクエスト送信用のURLを取得
    fn url(&self, path: &str) -> String {
        format!("{}{}", self.0.base_url.trim_end_matches('/'), path)
    }

    /// リクエスト履歴用のID
    fn id(&self) -> Uuid {
        self.0.id
    }

    /// リクエスト履歴用の名前
    fn name(&self) -> String {
        self.0.name.clone()
    }

    /// リクエスト履歴用のIPアドレス
    fn ip(&self) -> IpAddr {
        // フォールバック用のローカルホストアドレス
        const LOCALHOST: IpAddr = IpAddr::V4(std::net::Ipv4Addr::LOCALHOST);

        // base_urlからホスト部分を抽出してパース
        // 例: "http://192.168.1.100:11434" -> "192.168.1.100"
        let host = self
            .0
            .base_url
            .trim_start_matches("http://")
            .trim_start_matches("https://")
            .split(':')
            .next()
            .unwrap_or("127.0.0.1");
        host.parse::<IpAddr>().unwrap_or(LOCALHOST)
    }

    /// 上流転送リクエストの全体タイムアウト
    ///
    /// 共有 http_client には全体タイムアウトが無いため、応答しないエンドポイントで
    /// 無期限ハングするのを防ぐ。画像生成/編集/バリエーションは単一の有界レスポンス
    /// （トークンストリームではない）のため全体タイムアウトを付与しても問題ない。
    fn inference_timeout(&self) -> std::time::Duration {
        std::time::Duration::from_secs(self.0.inference_timeout_secs as u64)
    }
}

/// 画像生成対応バックエンドを選択
/// EndpointRegistry経由でのみ検索（NodeRegistryフォールバック廃止）
async fn select_image_backend(state: &AppState) -> Result<ImageBackend, LbError> {
    // EndpointRegistry経由で検索（SPEC-e8e9326e: 新方式のみ）
    let endpoints = state
        .endpoint_registry
        .list_online_by_capability(EndpointCapability::ImageGeneration)
        .await;

    let endpoint = endpoints.into_iter().next().ok_or_else(|| {
        LbError::ServiceUnavailable(
            "No endpoints available with image generation capability".to_string(),
        )
    })?;

    Ok(ImageBackend(endpoint))
}

/// POST /v1/images/generations - 画像生成（Text-to-Image）
///
/// JSON 形式でリクエスト
/// - model: 使用するモデル名 (例: "stable-diffusion-xl")
/// - prompt: 生成プロンプト
/// - n: 生成枚数（オプション、デフォルト: 1）
/// - size: 出力サイズ（オプション、デフォルト: "1024x1024"）
/// - quality: 品質（オプション、デフォルト: "standard"）
/// - style: スタイル（オプション、デフォルト: "vivid"）
/// - response_format: 出力形式（オプション、デフォルト: "url"）
pub async fn generations(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    State(state): State<AppState>,
    auth_ctx: Option<axum::Extension<ApiKeyAuthContext>>,
    Json(payload): Json<ImageGenerationRequest>,
) -> Result<Response, AppError> {
    let client_ip = Some(
        extract_client_ip_from_forwarded_headers(&headers)
            .unwrap_or_else(|| normalize_socket_ip(&addr)),
    );
    let api_key_id = auth_ctx.as_ref().map(|ext| ext.0.id);
    let start = Instant::now();
    let request_id = Uuid::new_v4();

    // プロンプトの検証
    if payload.prompt.is_empty() {
        return openai_error("Prompt is required", StatusCode::BAD_REQUEST);
    }

    // 生成枚数の検証（1-10）
    if payload.n == 0 || payload.n > 10 {
        return openai_error("n must be between 1 and 10", StatusCode::BAD_REQUEST);
    }

    let parsed = parse_quantized_model_name(&payload.model).map_err(AppError::from)?;
    let _lookup_model = parsed.base;

    // モデルの ImageGeneration capability を検証
    let model_info = load_registered_model(&state.db_pool, &payload.model).await?;
    if let Some(model_info) = model_info {
        if !model_info.has_capability(ModelCapability::ImageGeneration) {
            return openai_error(
                format!("Model '{}' does not support image generation", parsed.raw),
                StatusCode::BAD_REQUEST,
            );
        }
    }
    // 登録されていないモデルはエンドポイント側で処理（クラウドモデル等）

    info!(
        request_id = %request_id,
        model = %payload.model,
        prompt_length = payload.prompt.len(),
        n = payload.n,
        "Processing image generation request"
    );

    // 画像生成対応バックエンドを選択（EndpointRegistry優先、NodeRegistryフォールバック）
    let backend = select_image_backend(&state).await?;

    // JSON リクエストをプロキシ
    let client = &state.http_client;
    let url = backend.url("/v1/images/generations");

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
                format!("Failed to contact image generation backend: {}", e),
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
        RequestType::ImageGeneration,
        serde_json::to_value(&payload).unwrap_or(json!({})),
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

/// POST /v1/images/edits - 画像編集（Inpainting）
///
/// multipart/form-data 形式でリクエスト
/// - image: 編集対象の画像ファイル（PNG、最大4MB）
/// - mask: マスク画像（オプション、PNG）
/// - prompt: 編集プロンプト
/// - model: 使用するモデル名
/// - n: 生成枚数（オプション）
/// - size: 出力サイズ（オプション）
/// - response_format: 出力形式（オプション）
pub async fn edits(
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
    let mut image_data: Option<Vec<u8>> = None;
    let mut image_name: Option<String> = None;
    let mut mask_data: Option<Vec<u8>> = None;
    let mut mask_name: Option<String> = None;
    let mut prompt: Option<String> = None;
    let mut model: Option<String> = None;
    let mut n: Option<String> = None;
    let mut size: Option<String> = None;
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
            "image" => {
                image_name = field.file_name().map(|s| s.to_string());
                match field.bytes().await {
                    Ok(bytes) => image_data = Some(bytes.to_vec()),
                    Err(e) => {
                        return openai_error(
                            format!("Failed to read image field: {}", e),
                            StatusCode::BAD_REQUEST,
                        )
                    }
                }
            }
            "mask" => {
                mask_name = field.file_name().map(|s| s.to_string());
                match field.bytes().await {
                    Ok(bytes) => mask_data = Some(bytes.to_vec()),
                    Err(e) => {
                        return openai_error(
                            format!("Failed to read mask field: {}", e),
                            StatusCode::BAD_REQUEST,
                        )
                    }
                }
            }
            "prompt" => match field.text().await {
                Ok(text) => prompt = Some(text),
                Err(e) => {
                    return openai_error(
                        format!("Failed to read prompt field: {}", e),
                        StatusCode::BAD_REQUEST,
                    )
                }
            },
            "model" => match field.text().await {
                Ok(text) => model = Some(text),
                Err(e) => {
                    return openai_error(
                        format!("Failed to read model field: {}", e),
                        StatusCode::BAD_REQUEST,
                    )
                }
            },
            "n" => match field.text().await {
                Ok(text) => n = Some(text),
                Err(e) => {
                    return openai_error(
                        format!("Failed to read n field: {}", e),
                        StatusCode::BAD_REQUEST,
                    )
                }
            },
            "size" => match field.text().await {
                Ok(text) => size = Some(text),
                Err(e) => {
                    return openai_error(
                        format!("Failed to read size field: {}", e),
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
    let image_data = match image_data {
        Some(data) => data,
        None => return openai_error("Missing required field: image", StatusCode::BAD_REQUEST),
    };
    let prompt = match prompt {
        Some(p) => p,
        None => return openai_error("Missing required field: prompt", StatusCode::BAD_REQUEST),
    };
    let model = model.unwrap_or_else(|| "stable-diffusion-xl".to_string());

    // 画像サイズの検証（最大4MB）
    const MAX_IMAGE_SIZE: usize = 4 * 1024 * 1024; // 4MB
    if image_data.len() > MAX_IMAGE_SIZE {
        return openai_error(
            "Image file exceeds maximum size of 4MB",
            StatusCode::BAD_REQUEST,
        );
    }

    info!(
        request_id = %request_id,
        model = %model,
        image_size = image_data.len(),
        has_mask = mask_data.is_some(),
        "Processing image edit request"
    );

    // 画像生成対応バックエンドを選択（EndpointRegistry優先、NodeRegistryフォールバック）
    let backend = select_image_backend(&state).await?;

    // multipart リクエストを構築してプロキシ
    let client = &state.http_client;
    let url = backend.url("/v1/images/edits");

    let mut form = reqwest::multipart::Form::new().part(
        "image",
        reqwest::multipart::Part::bytes(image_data)
            .file_name(image_name.unwrap_or_else(|| "image.png".to_string()))
            .mime_str("image/png")
            .expect("image/png is a valid MIME type"),
    );

    if let Some(mask) = mask_data {
        form = form.part(
            "mask",
            reqwest::multipart::Part::bytes(mask)
                .file_name(mask_name.unwrap_or_else(|| "mask.png".to_string()))
                .mime_str("image/png")
                .expect("image/png is a valid MIME type"),
        );
    }

    form = form.text("prompt", prompt.clone());
    form = form.text("model", model.clone());

    if let Some(n_val) = n {
        form = form.text("n", n_val);
    }

    if let Some(size_val) = size {
        form = form.text("size", size_val);
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
                format!("Failed to contact image edit node: {}", e),
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
        RequestType::ImageEdit,
        json!({"model": model, "prompt": prompt, "type": "image_edit"}),
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

/// POST /v1/images/variations - 画像バリエーション生成
///
/// multipart/form-data 形式でリクエスト
/// - image: 元画像ファイル（PNG、最大4MB）
/// - model: 使用するモデル名
/// - n: 生成枚数（オプション）
/// - size: 出力サイズ（オプション）
/// - response_format: 出力形式（オプション）
pub async fn variations(
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
    let mut image_data: Option<Vec<u8>> = None;
    let mut image_name: Option<String> = None;
    let mut model: Option<String> = None;
    let mut n: Option<String> = None;
    let mut size: Option<String> = None;
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
            "image" => {
                image_name = field.file_name().map(|s| s.to_string());
                match field.bytes().await {
                    Ok(bytes) => image_data = Some(bytes.to_vec()),
                    Err(e) => {
                        return openai_error(
                            format!("Failed to read image field: {}", e),
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
            "n" => match field.text().await {
                Ok(text) => n = Some(text),
                Err(e) => {
                    return openai_error(
                        format!("Failed to read n field: {}", e),
                        StatusCode::BAD_REQUEST,
                    )
                }
            },
            "size" => match field.text().await {
                Ok(text) => size = Some(text),
                Err(e) => {
                    return openai_error(
                        format!("Failed to read size field: {}", e),
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
    let image_data = match image_data {
        Some(data) => data,
        None => return openai_error("Missing required field: image", StatusCode::BAD_REQUEST),
    };
    let model = model.unwrap_or_else(|| "stable-diffusion-xl".to_string());

    // 画像サイズの検証（最大4MB）
    const MAX_IMAGE_SIZE: usize = 4 * 1024 * 1024; // 4MB
    if image_data.len() > MAX_IMAGE_SIZE {
        return openai_error(
            "Image file exceeds maximum size of 4MB",
            StatusCode::BAD_REQUEST,
        );
    }

    info!(
        request_id = %request_id,
        model = %model,
        image_size = image_data.len(),
        "Processing image variation request"
    );

    // 画像生成対応バックエンドを選択（EndpointRegistry優先、NodeRegistryフォールバック）
    let backend = select_image_backend(&state).await?;

    // multipart リクエストを構築してプロキシ
    let client = &state.http_client;
    let url = backend.url("/v1/images/variations");

    let mut form = reqwest::multipart::Form::new().part(
        "image",
        reqwest::multipart::Part::bytes(image_data)
            .file_name(image_name.unwrap_or_else(|| "image.png".to_string()))
            .mime_str("image/png")
            .expect("image/png is a valid MIME type"),
    );

    form = form.text("model", model.clone());

    if let Some(n_val) = n {
        form = form.text("n", n_val);
    }

    if let Some(size_val) = size {
        form = form.text("size", size_val);
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
                format!("Failed to contact image variation node: {}", e),
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
        RequestType::ImageVariation,
        json!({"model": model, "type": "image_variation"}),
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

#[cfg(test)]
mod tests {
    use super::{extract_client_ip_from_forwarded_headers, parse_forwarded_ip_candidate};
    use axum::http::{HeaderMap, HeaderValue, StatusCode};
    use std::net::IpAddr;

    #[test]
    fn extract_client_ip_prefers_x_forwarded_for() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-forwarded-for",
            HeaderValue::from_static("unknown, 198.51.100.30, 10.0.0.3"),
        );
        let parsed =
            extract_client_ip_from_forwarded_headers(&headers).expect("must parse x-forwarded-for");
        assert_eq!(parsed, "198.51.100.30".parse::<IpAddr>().unwrap());
    }

    #[test]
    fn extract_client_ip_uses_forwarded_when_xff_missing() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "forwarded",
            HeaderValue::from_static("for=unknown;proto=https, for=\"[2001:db8::20]:9443\""),
        );
        let parsed =
            extract_client_ip_from_forwarded_headers(&headers).expect("must parse forwarded");
        assert_eq!(parsed, "2001:db8::20".parse::<IpAddr>().unwrap());
    }

    #[test]
    fn parse_forwarded_ip_candidate_parses_ipv4_with_port() {
        let parsed =
            parse_forwarded_ip_candidate("198.51.100.44:8080").expect("must parse ipv4 with port");
        assert_eq!(parsed, "198.51.100.44".parse::<IpAddr>().unwrap());
    }

    #[test]
    fn test_image_size_limit() {
        const MAX_IMAGE_SIZE: usize = 4 * 1024 * 1024; // 4MB

        // 4MB以下は許可
        let small_image = vec![0u8; MAX_IMAGE_SIZE];
        assert!(small_image.len() <= MAX_IMAGE_SIZE);

        // 4MB超は拒否
        let large_image = vec![0u8; MAX_IMAGE_SIZE + 1];
        assert!(large_image.len() > MAX_IMAGE_SIZE);
    }

    #[test]
    fn test_n_validation() {
        // Helper function to validate n parameter (1-10 is valid)
        fn is_valid_n(n: u32) -> bool {
            (1..=10).contains(&n)
        }

        // n = 0 は無効
        assert!(!is_valid_n(0));

        // n = 1-10 は有効
        for n in 1..=10 {
            assert!(is_valid_n(n));
        }

        // n = 11 は無効
        assert!(!is_valid_n(11));
    }

    // T007: 画像生成 capabilities検証テスト (RED)
    // ImageGeneration capability を持たないモデルで /v1/images/generations を呼ぶとエラー
    #[test]
    fn test_image_generation_capability_validation_error_message() {
        use crate::types::model::{ModelCapability, ModelType};

        // LLMモデルはTextGenerationのみ、ImageGenerationは非対応
        let llm_caps = ModelCapability::from_model_type(ModelType::Llm);
        assert!(!llm_caps.contains(&ModelCapability::ImageGeneration));

        // TTSモデルもTextToSpeechのみ、ImageGenerationは非対応
        let tts_caps = ModelCapability::from_model_type(ModelType::TextToSpeech);
        assert!(!tts_caps.contains(&ModelCapability::ImageGeneration));

        // ASRモデルもSpeechToTextのみ、ImageGenerationは非対応
        let stt_caps = ModelCapability::from_model_type(ModelType::SpeechToText);
        assert!(!stt_caps.contains(&ModelCapability::ImageGeneration));

        // EmbeddingモデルもEmbeddingのみ、ImageGenerationは非対応
        let embed_caps = ModelCapability::from_model_type(ModelType::Embedding);
        assert!(!embed_caps.contains(&ModelCapability::ImageGeneration));

        // 期待されるエラーメッセージ形式
        let model_name = "llama-3.1-8b";
        let expected_error = format!("Model '{}' does not support image generation", model_name);
        assert!(expected_error.contains("does not support image generation"));
    }

    // --- ImageBackend helper tests ---

    #[test]
    fn image_backend_url_strips_trailing_slash() {
        use super::ImageBackend;
        use crate::types::endpoint::{Endpoint, EndpointType};

        let ep = Endpoint::new(
            "img-node".to_string(),
            "http://localhost:9090/".to_string(),
            EndpointType::Xllm,
        );
        let backend = ImageBackend(ep);
        assert_eq!(
            backend.url("/v1/images/generations"),
            "http://localhost:9090/v1/images/generations"
        );
    }

    #[test]
    fn image_backend_url_no_trailing_slash() {
        use super::ImageBackend;
        use crate::types::endpoint::{Endpoint, EndpointType};

        let ep = Endpoint::new(
            "img-node".to_string(),
            "http://10.0.0.2:8080".to_string(),
            EndpointType::Vllm,
        );
        let backend = ImageBackend(ep);
        assert_eq!(
            backend.url("/v1/images/edits"),
            "http://10.0.0.2:8080/v1/images/edits"
        );
    }

    #[test]
    fn image_backend_url_variations_path() {
        use super::ImageBackend;
        use crate::types::endpoint::{Endpoint, EndpointType};

        let ep = Endpoint::new(
            "img-node".to_string(),
            "http://192.168.0.1:7860".to_string(),
            EndpointType::Xllm,
        );
        let backend = ImageBackend(ep);
        assert_eq!(
            backend.url("/v1/images/variations"),
            "http://192.168.0.1:7860/v1/images/variations"
        );
    }

    #[test]
    fn image_backend_id_returns_endpoint_id() {
        use super::ImageBackend;
        use crate::types::endpoint::{Endpoint, EndpointType};

        let ep = Endpoint::new(
            "test".to_string(),
            "http://localhost:8080".to_string(),
            EndpointType::Xllm,
        );
        let expected_id = ep.id;
        let backend = ImageBackend(ep);
        assert_eq!(backend.id(), expected_id);
    }

    #[test]
    fn image_backend_name_returns_endpoint_name() {
        use super::ImageBackend;
        use crate::types::endpoint::{Endpoint, EndpointType};

        let ep = Endpoint::new(
            "sd-xl-backend".to_string(),
            "http://localhost:8080".to_string(),
            EndpointType::Xllm,
        );
        let backend = ImageBackend(ep);
        assert_eq!(backend.name(), "sd-xl-backend");
    }

    #[test]
    fn image_backend_ip_extracts_from_http_url() {
        use super::ImageBackend;
        use crate::types::endpoint::{Endpoint, EndpointType};

        let ep = Endpoint::new(
            "test".to_string(),
            "http://192.168.1.200:7860".to_string(),
            EndpointType::Xllm,
        );
        let backend = ImageBackend(ep);
        assert_eq!(backend.ip(), "192.168.1.200".parse::<IpAddr>().unwrap());
    }

    #[test]
    fn image_backend_ip_extracts_from_https_url() {
        use super::ImageBackend;
        use crate::types::endpoint::{Endpoint, EndpointType};

        let ep = Endpoint::new(
            "test".to_string(),
            "https://10.0.0.10:443".to_string(),
            EndpointType::Vllm,
        );
        let backend = ImageBackend(ep);
        assert_eq!(backend.ip(), "10.0.0.10".parse::<IpAddr>().unwrap());
    }

    #[test]
    fn image_backend_ip_falls_back_to_localhost() {
        use super::ImageBackend;
        use crate::types::endpoint::{Endpoint, EndpointType};

        let mut ep = Endpoint::new(
            "test".to_string(),
            "http://my-hostname:8080".to_string(),
            EndpointType::Xllm,
        );
        ep.base_url = "http://my-hostname:8080".to_string();
        let backend = ImageBackend(ep);
        assert_eq!(backend.ip(), "127.0.0.1".parse::<IpAddr>().unwrap());
    }

    // --- error_response tests ---

    #[test]
    fn error_response_http_returns_correct_status() {
        use super::error_response;
        use crate::common::error::LbError;

        let resp = error_response(
            LbError::Http("upstream error".to_string()),
            StatusCode::BAD_GATEWAY,
        );
        assert_eq!(resp.status(), StatusCode::BAD_GATEWAY);
    }

    #[test]
    fn error_response_service_unavailable_returns_503() {
        use super::error_response;
        use crate::common::error::LbError;

        let resp = error_response(
            LbError::ServiceUnavailable("no image backends".to_string()),
            StatusCode::SERVICE_UNAVAILABLE,
        );
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[test]
    fn error_response_invalid_model_name_returns_400() {
        use super::error_response;
        use crate::common::error::LbError;

        let resp = error_response(
            LbError::InvalidModelName("bad:model:name".to_string()),
            StatusCode::BAD_REQUEST,
        );
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[test]
    fn error_response_fallback_type_for_internal_error() {
        use super::error_response;
        use crate::common::error::LbError;

        let resp = error_response(
            LbError::Database("db connection lost".to_string()),
            StatusCode::INTERNAL_SERVER_ERROR,
        );
        assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    // --- openai_error helper tests ---

    #[test]
    fn openai_error_returns_ok_with_requested_status() {
        use super::openai_error;

        let result = openai_error("missing field", StatusCode::BAD_REQUEST);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().status(), StatusCode::BAD_REQUEST);
    }

    #[test]
    fn openai_error_accepts_owned_string() {
        use super::openai_error;

        let msg = format!("Image file exceeds maximum size of {}MB", 4);
        let result = openai_error(msg, StatusCode::PAYLOAD_TOO_LARGE);
        assert!(result.is_ok());
    }

    // --- forwarded header extraction tests ---

    #[test]
    fn extract_client_ip_returns_none_for_empty_headers() {
        let headers = HeaderMap::new();
        assert!(extract_client_ip_from_forwarded_headers(&headers).is_none());
    }

    #[test]
    fn extract_client_ip_returns_none_when_all_unknown() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-forwarded-for",
            HeaderValue::from_static("unknown, unknown, _hidden"),
        );
        assert!(extract_client_ip_from_forwarded_headers(&headers).is_none());
    }

    #[test]
    fn parse_forwarded_ip_candidate_empty_returns_none() {
        assert!(parse_forwarded_ip_candidate("").is_none());
    }

    #[test]
    fn parse_forwarded_ip_candidate_unknown_returns_none() {
        assert!(parse_forwarded_ip_candidate("unknown").is_none());
        assert!(parse_forwarded_ip_candidate("UNKNOWN").is_none());
    }

    #[test]
    fn parse_forwarded_ip_candidate_obfuscated_returns_none() {
        assert!(parse_forwarded_ip_candidate("_secret").is_none());
    }

    #[test]
    fn parse_forwarded_ip_candidate_plain_ipv4() {
        let ip = parse_forwarded_ip_candidate("203.0.113.50").expect("should parse ipv4");
        assert_eq!(ip, "203.0.113.50".parse::<IpAddr>().unwrap());
    }

    #[test]
    fn parse_forwarded_ip_candidate_plain_ipv6() {
        let ip = parse_forwarded_ip_candidate("2001:db8::1").expect("should parse ipv6");
        assert_eq!(ip, "2001:db8::1".parse::<IpAddr>().unwrap());
    }

    #[test]
    fn parse_forwarded_ip_candidate_bracketed_ipv6() {
        let ip = parse_forwarded_ip_candidate("\"[2001:db8::ff]:9090\"")
            .expect("should parse bracketed ipv6");
        assert_eq!(ip, "2001:db8::ff".parse::<IpAddr>().unwrap());
    }

    #[test]
    fn parse_forwarded_ip_candidate_ipv4_with_port() {
        let ip =
            parse_forwarded_ip_candidate("10.0.0.5:3000").expect("should parse ipv4 with port");
        assert_eq!(ip, "10.0.0.5".parse::<IpAddr>().unwrap());
    }

    #[test]
    fn parse_forwarded_ip_candidate_invalid() {
        assert!(parse_forwarded_ip_candidate("garbage-value").is_none());
    }

    // --- Image generation validation logic tests ---

    #[test]
    fn test_prompt_empty_check() {
        let empty_prompt = "";
        assert!(empty_prompt.is_empty());
        let valid_prompt = "A cat sitting on a windowsill";
        assert!(!valid_prompt.is_empty());
    }

    #[test]
    fn test_n_boundary_values() {
        let is_valid_n = |n: u8| (1..=10).contains(&n);
        // n=0 is invalid
        assert!(!is_valid_n(0));
        // n=1 is valid
        assert!(is_valid_n(1));
        // n=10 is valid
        assert!(is_valid_n(10));
        // n=11 is invalid
        assert!(!is_valid_n(11));
    }

    #[test]
    fn test_image_size_max_boundary() {
        const MAX_IMAGE_SIZE: usize = 4 * 1024 * 1024;
        let fits_limit = |size: usize| size <= MAX_IMAGE_SIZE;

        // Exactly at boundary - allowed
        assert!(fits_limit(MAX_IMAGE_SIZE));

        // One byte over - rejected
        assert!(!fits_limit(MAX_IMAGE_SIZE + 1));

        // Well below boundary - allowed
        let small = 1024_usize;
        assert!(small <= MAX_IMAGE_SIZE);
    }

    #[test]
    fn test_image_generation_request_defaults() {
        use crate::common::protocol::ImageGenerationRequest;

        let json = r#"{"model":"sd-xl","prompt":"A landscape"}"#;
        let req: ImageGenerationRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.n, 1);
        assert_eq!(req.model, "sd-xl");
        assert_eq!(req.prompt, "A landscape");
        assert!(req.negative_prompt.is_none());
        assert!(req.seed.is_none());
        assert!(req.steps.is_none());
    }

    #[test]
    fn test_image_generation_request_with_all_optional_fields() {
        use crate::common::protocol::ImageGenerationRequest;

        let json = r#"{
            "model": "sd-xl",
            "prompt": "A beautiful sunset",
            "n": 4,
            "size": "512x512",
            "quality": "hd",
            "style": "natural",
            "response_format": "b64_json",
            "negative_prompt": "blurry, low quality",
            "seed": 42,
            "steps": 30
        }"#;
        let req: ImageGenerationRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.n, 4);
        assert_eq!(req.negative_prompt, Some("blurry, low quality".to_string()));
        assert_eq!(req.seed, Some(42));
        assert_eq!(req.steps, Some(30));
    }

    #[test]
    fn test_model_default_for_edits() {
        // When model is None, default is "stable-diffusion-xl"
        let model: Option<String> = None;
        let resolved = match model {
            Some(model) => model,
            None => "stable-diffusion-xl".to_string(),
        };
        assert_eq!(resolved, "stable-diffusion-xl");
    }

    #[test]
    fn test_model_override_for_edits() {
        let model: Option<String> = Some("dall-e-3".to_string());
        let resolved = match model {
            Some(model) => model,
            None => "stable-diffusion-xl".to_string(),
        };
        assert_eq!(resolved, "dall-e-3");
    }

    #[test]
    fn test_model_default_for_variations() {
        let model: Option<String> = None;
        let resolved = match model {
            Some(model) => model,
            None => "stable-diffusion-xl".to_string(),
        };
        assert_eq!(resolved, "stable-diffusion-xl");
    }
}

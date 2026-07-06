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
use std::net::SocketAddr;
use std::time::Instant;
use tracing::info;
use uuid::Uuid;

use crate::{
    api::{
        error::AppError,
        inference_backend::InferenceBackend,
        model_name::parse_quantized_model_name,
        models::load_registered_model,
        proxy::{forward_streaming_response, save_request_record},
    },
    auth::middleware::ApiKeyAuthContext,
    common::ip::normalize_socket_ip,
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

/// 画像生成対応バックエンドを選択
/// EndpointRegistry経由でのみ検索（NodeRegistryフォールバック廃止）
async fn select_image_backend(state: &AppState) -> Result<InferenceBackend, LbError> {
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

    Ok(InferenceBackend(endpoint))
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
        crate::common::ip::extract_client_ip_from_headers(&headers)
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
        crate::common::ip::extract_client_ip_from_headers(&headers)
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
        crate::common::ip::extract_client_ip_from_headers(&headers)
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
mod tests;

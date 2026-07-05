//! 画像生成 DTO（text-to-image, edits, variations, response）
//!
//! arch-review [H6] round2: common/protocol.rs から画像生成 DTO を分離。

use crate::types::media::{ImageQuality, ImageResponseFormat, ImageSize, ImageStyle};
use serde::{Deserialize, Serialize};

/// 画像生成リクエスト (Text-to-Image)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ImageGenerationRequest {
    /// モデル名 (例: "stable-diffusion-xl", "dall-e-3")
    pub model: String,
    /// 生成プロンプト
    pub prompt: String,
    /// 生成画像数 (1-10、デフォルト1)
    #[serde(default = "default_image_n")]
    pub n: u8,
    /// 出力サイズ
    #[serde(default)]
    pub size: ImageSize,
    /// 品質設定
    #[serde(default)]
    pub quality: ImageQuality,
    /// スタイル
    #[serde(default)]
    pub style: ImageStyle,
    /// レスポンスフォーマット
    #[serde(default)]
    pub response_format: ImageResponseFormat,
    /// ネガティブプロンプト（SD拡張）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub negative_prompt: Option<String>,
    /// シード値（再現性用）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed: Option<i64>,
    /// 生成ステップ数（SD拡張、デフォルト: 20）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub steps: Option<u32>,
}

fn default_image_n() -> u8 {
    1
}

/// 画像編集リクエスト (Inpainting)
///
/// multipart/form-dataとして送信されるため、画像データは別途処理
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ImageEditRequest {
    /// モデル名
    pub model: String,
    /// 編集プロンプト
    pub prompt: String,
    /// 生成画像数 (1-10、デフォルト1)
    #[serde(default = "default_image_n")]
    pub n: u8,
    /// 出力サイズ
    #[serde(default)]
    pub size: ImageSize,
    /// レスポンスフォーマット
    #[serde(default)]
    pub response_format: ImageResponseFormat,
}

/// 画像バリエーションリクエスト
///
/// multipart/form-dataとして送信されるため、画像データは別途処理
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ImageVariationRequest {
    /// モデル名
    pub model: String,
    /// 生成画像数 (1-10、デフォルト1)
    #[serde(default = "default_image_n")]
    pub n: u8,
    /// 出力サイズ
    #[serde(default)]
    pub size: ImageSize,
    /// レスポンスフォーマット
    #[serde(default)]
    pub response_format: ImageResponseFormat,
}

/// 画像レスポンス (generations/edits/variations共通)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ImageResponse {
    /// 生成時刻 (Unix timestamp)
    pub created: i64,
    /// 生成された画像データ配列
    pub data: Vec<ImageData>,
}

/// 画像データ
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum ImageData {
    /// URL形式
    Url {
        /// 画像URL
        url: String,
        /// 改訂されたプロンプト（DALL-E 3等）
        #[serde(skip_serializing_if = "Option::is_none")]
        revised_prompt: Option<String>,
    },
    /// Base64形式
    Base64 {
        /// Base64エンコードされた画像データ
        b64_json: String,
        /// 改訂されたプロンプト
        #[serde(skip_serializing_if = "Option::is_none")]
        revised_prompt: Option<String>,
    },
}

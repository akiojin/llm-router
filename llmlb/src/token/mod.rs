//! トークン抽出モジュール
//!
//! OpenAI互換レスポンスからトークン数を抽出し、
//! usageフィールドがない場合はtiktokenで推定する。

use serde_json::Value;
use tiktoken_rs::cl100k_base;

/// トークン使用量
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TokenUsage {
    /// 入力トークン数
    pub input_tokens: Option<u32>,
    /// 出力トークン数
    pub output_tokens: Option<u32>,
    /// 総トークン数
    pub total_tokens: Option<u32>,
}

impl TokenUsage {
    /// 新しいTokenUsageを作成
    pub fn new(input: Option<u32>, output: Option<u32>, total: Option<u32>) -> Self {
        Self {
            input_tokens: input,
            output_tokens: output,
            total_tokens: total,
        }
    }

    /// 空のTokenUsageかどうか
    pub fn is_empty(&self) -> bool {
        self.input_tokens.is_none() && self.output_tokens.is_none() && self.total_tokens.is_none()
    }
}

/// SSEストリーミングレスポンスのトークン累積器
///
/// OpenAI互換のSSEストリーミングレスポンスをパースし、
/// チャンクごとにコンテンツを累積してトークン使用量を計算する
#[derive(Debug)]
pub struct StreamingTokenAccumulator {
    /// モデル名（トークン推定用）
    model: String,
    /// 累積されたコンテンツ
    accumulated_content: String,
    /// 入力トークン数（リクエスト時に設定可能）
    input_tokens: Option<u32>,
    /// 抽出されたusageフィールド（最終チャンクから）
    extracted_usage: Option<TokenUsage>,
    /// ストリーム完了フラグ
    done: bool,
}

impl StreamingTokenAccumulator {
    /// 新しいStreamingTokenAccumulatorを作成
    pub fn new(model: &str) -> Self {
        Self {
            model: model.to_string(),
            accumulated_content: String::new(),
            input_tokens: None,
            extracted_usage: None,
            done: false,
        }
    }

    /// 入力トークン数を設定
    pub fn set_input_tokens(&mut self, tokens: Option<u32>) {
        self.input_tokens = tokens;
    }

    /// SSEチャンクを処理
    pub fn process_chunk(&mut self, chunk: &str) {
        // 空行やコメント行はスキップ
        let chunk = chunk.trim();
        if chunk.is_empty() || chunk.starts_with(':') {
            return;
        }

        // "data: " プレフィックスを除去
        let data = if let Some(stripped) = chunk.strip_prefix("data: ") {
            stripped
        } else if let Some(stripped) = chunk.strip_prefix("data:") {
            stripped.trim()
        } else {
            return;
        };

        // [DONE] マーカーをチェック
        if data == "[DONE]" {
            self.done = true;
            return;
        }

        // JSONパース
        if let Ok(json) = serde_json::from_str::<Value>(data) {
            // usageフィールドを抽出（最終チャンクに含まれる場合がある）
            if let Some(usage) = extract_usage_from_response(&json) {
                self.extracted_usage = Some(usage);
            }

            // delta.contentを抽出して累積
            if let Some(choices) = json.get("choices").and_then(|c| c.as_array()) {
                for choice in choices {
                    if let Some(content) = choice
                        .get("delta")
                        .and_then(|d| d.get("content"))
                        .and_then(|c| c.as_str())
                    {
                        self.accumulated_content.push_str(content);
                    }
                }
            }

            // Open Responses APIのストリーミング形式（response.output_text.*）にも対応
            if let Some(event_type) = json.get("type").and_then(|t| t.as_str()) {
                match event_type {
                    "response.output_text.delta" => {
                        if let Some(delta) = json.get("delta").and_then(|d| d.as_str()) {
                            self.accumulated_content.push_str(delta);
                        }
                    }
                    // deltaイベントが欠落している場合のみdone.textを利用
                    "response.output_text.done" if self.accumulated_content.is_empty() => {
                        if let Some(text) = json.get("text").and_then(|t| t.as_str()) {
                            self.accumulated_content.push_str(text);
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    /// 累積されたコンテンツを取得
    pub fn accumulated_content(&self) -> &str {
        &self.accumulated_content
    }

    /// ストリームが完了したかどうか
    pub fn is_done(&self) -> bool {
        self.done
    }

    /// 最終的なTokenUsageを計算
    pub fn finalize(&self) -> TokenUsage {
        // usageフィールドが抽出されている場合はそれを使用
        if let Some(ref usage) = self.extracted_usage {
            return usage.clone();
        }

        // usageがない場合はtiktokenで推定
        let output_tokens = if self.accumulated_content.is_empty() {
            Some(0)
        } else {
            estimate_tokens(&self.accumulated_content, &self.model)
        };

        let input_tokens = self.input_tokens;

        // total_tokensを計算
        let total_tokens = match (input_tokens, output_tokens) {
            (Some(i), Some(o)) => Some(i + o),
            (Some(i), None) => Some(i),
            (None, Some(o)) => Some(o),
            (None, None) => None,
        };

        TokenUsage::new(input_tokens, output_tokens, total_tokens)
    }
}

/// OpenAI互換レスポンスのusageフィールドからトークン数を抽出
///
/// # Arguments
/// * `response_body` - OpenAI互換APIレスポンスのJSON
///
/// # Returns
/// * `Some(TokenUsage)` - usageフィールドが存在する場合
/// * `None` - usageフィールドが存在しない場合
pub fn extract_usage_from_response(response_body: &Value) -> Option<TokenUsage> {
    let usage = response_body
        .get("usage")
        .or_else(|| response_body.get("response").and_then(|r| r.get("usage")))?;

    // OpenAI互換（prompt/completion）とResponses API（input/output）の両方に対応
    let input_tokens = usage
        .get("prompt_tokens")
        .or_else(|| usage.get("input_tokens"))
        .and_then(|v| v.as_u64())
        .map(|v| v as u32);

    let output_tokens = usage
        .get("completion_tokens")
        .or_else(|| usage.get("output_tokens"))
        .and_then(|v| v.as_u64())
        .map(|v| v as u32);

    let total_tokens = usage
        .get("total_tokens")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32);

    Some(TokenUsage::new(input_tokens, output_tokens, total_tokens))
}

/// tiktokenを使用してテキストのトークン数を推定
///
/// # Arguments
/// * `text` - トークン数を推定するテキスト
/// * `_model` - モデル名（現在は未使用、将来的にモデル別エンコーディングに対応）
///
/// # Returns
/// * `Some(u32)` - 推定トークン数
/// * `None` - 推定できない場合
pub fn estimate_tokens(text: &str, _model: &str) -> Option<u32> {
    // cl100k_base エンコーディングを使用（GPT-4, GPT-3.5-turbo互換）
    // llama系モデルも概ね近い値になるため、フォールバックとして使用
    let bpe = cl100k_base().ok()?;
    let tokens = bpe.encode_with_special_tokens(text);
    Some(tokens.len() as u32)
}

/// トークン抽出（usageフィールド優先、フォールバックでtiktoken推定）
///
/// # Arguments
/// * `response_body` - OpenAI互換APIレスポンスのJSON
/// * `request_text` - リクエストテキスト（フォールバック用）
/// * `response_text` - レスポンステキスト（フォールバック用）
/// * `model` - モデル名
///
/// # Returns
/// * `TokenUsage` - 抽出または推定されたトークン使用量
pub fn extract_or_estimate_tokens(
    response_body: &Value,
    request_text: Option<&str>,
    response_text: Option<&str>,
    model: &str,
) -> TokenUsage {
    // まずusageフィールドから抽出を試みる
    if let Some(usage) = extract_usage_from_response(response_body) {
        return usage;
    }

    // usageがない場合はtiktokenで推定
    let input_tokens = request_text.and_then(|text| estimate_tokens(text, model));
    let output_tokens = response_text.and_then(|text| estimate_tokens(text, model));

    // total_tokensは入力と出力の合計
    let total_tokens = match (input_tokens, output_tokens) {
        (Some(i), Some(o)) => Some(i + o),
        (Some(i), None) => Some(i),
        (None, Some(o)) => Some(o),
        (None, None) => None,
    };

    TokenUsage::new(input_tokens, output_tokens, total_tokens)
}

#[cfg(test)]
mod tests;

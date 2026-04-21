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
mod tests {
    use super::*;
    use serde_json::json;

    // T-3: usageフィールドからのトークン抽出テスト
    #[test]
    fn test_extract_usage_from_response_with_usage_field() {
        let response = json!({
            "id": "chatcmpl-123",
            "object": "chat.completion",
            "choices": [
                {
                    "message": {
                        "role": "assistant",
                        "content": "Hello!"
                    }
                }
            ],
            "usage": {
                "prompt_tokens": 10,
                "completion_tokens": 5,
                "total_tokens": 15
            }
        });

        let usage = extract_usage_from_response(&response);
        assert!(usage.is_some(), "usageフィールドがある場合はSomeを返すべき");
        let usage = usage.unwrap();
        assert_eq!(usage.input_tokens, Some(10));
        assert_eq!(usage.output_tokens, Some(5));
        assert_eq!(usage.total_tokens, Some(15));
    }

    #[test]
    fn test_extract_usage_from_response_without_usage_field() {
        let response = json!({
            "id": "chatcmpl-123",
            "object": "chat.completion",
            "choices": [
                {
                    "message": {
                        "role": "assistant",
                        "content": "Hello!"
                    }
                }
            ]
        });

        let usage = extract_usage_from_response(&response);
        assert!(usage.is_none(), "usageフィールドがない場合はNoneを返すべき");
    }

    #[test]
    fn test_extract_usage_from_response_partial_usage() {
        // prompt_tokensのみの場合
        let response = json!({
            "usage": {
                "prompt_tokens": 10
            }
        });

        let usage = extract_usage_from_response(&response);
        assert!(usage.is_some());
        let usage = usage.unwrap();
        assert_eq!(usage.input_tokens, Some(10));
        assert_eq!(usage.output_tokens, None);
        assert_eq!(usage.total_tokens, None);
    }

    #[test]
    fn test_extract_usage_from_response_with_responses_api_usage_field() {
        let response = json!({
            "usage": {
                "input_tokens": 12,
                "output_tokens": 34,
                "total_tokens": 46
            }
        });

        let usage = extract_usage_from_response(&response);
        assert!(usage.is_some());
        let usage = usage.unwrap();
        assert_eq!(usage.input_tokens, Some(12));
        assert_eq!(usage.output_tokens, Some(34));
        assert_eq!(usage.total_tokens, Some(46));
    }

    #[test]
    fn test_extract_usage_from_response_with_nested_response_usage() {
        let response = json!({
            "type": "response.done",
            "response": {
                "usage": {
                    "input_tokens": 7,
                    "output_tokens": 9,
                    "total_tokens": 16
                }
            }
        });

        let usage = extract_usage_from_response(&response);
        assert!(usage.is_some());
        let usage = usage.unwrap();
        assert_eq!(usage.input_tokens, Some(7));
        assert_eq!(usage.output_tokens, Some(9));
        assert_eq!(usage.total_tokens, Some(16));
    }

    // T-4: tiktoken推定テスト
    #[test]
    fn test_estimate_tokens_with_known_model() {
        let text = "Hello, world!";
        let model = "gpt-4";

        let tokens = estimate_tokens(text, model);
        assert!(
            tokens.is_some(),
            "既知のモデルではトークン数を推定できるべき"
        );
        // "Hello, world!" は通常4トークン程度
        let token_count = tokens.unwrap();
        assert!(
            token_count > 0 && token_count < 10,
            "トークン数は妥当な範囲内であるべき: {}",
            token_count
        );
    }

    #[test]
    fn test_estimate_tokens_with_llama_model() {
        let text = "こんにちは、世界！";
        let model = "llama-3.1-8b";

        let tokens = estimate_tokens(text, model);
        // llama系もcl100k_baseでフォールバック推定
        assert!(
            tokens.is_some(),
            "llama系モデルでもトークン推定が可能であるべき"
        );
    }

    #[test]
    fn test_estimate_tokens_empty_text() {
        let text = "";
        let model = "gpt-4";

        let tokens = estimate_tokens(text, model);
        assert!(tokens.is_some());
        assert_eq!(tokens.unwrap(), 0, "空文字は0トークン");
    }

    // T-5: usageフィールド欠如時のフォールバックテスト
    #[test]
    fn test_extract_or_estimate_with_usage_field() {
        let response = json!({
            "usage": {
                "prompt_tokens": 100,
                "completion_tokens": 50,
                "total_tokens": 150
            }
        });

        let usage =
            extract_or_estimate_tokens(&response, Some("What is 2+2?"), Some("2+2=4"), "gpt-4");

        // usageフィールドがある場合はそれを使用
        assert_eq!(usage.input_tokens, Some(100));
        assert_eq!(usage.output_tokens, Some(50));
        assert_eq!(usage.total_tokens, Some(150));
    }

    #[test]
    fn test_extract_or_estimate_fallback_to_tiktoken() {
        let response = json!({
            "choices": [{"message": {"content": "The answer is 4."}}]
        });

        let usage = extract_or_estimate_tokens(
            &response,
            Some("What is 2+2?"),
            Some("The answer is 4."),
            "gpt-4",
        );

        // usageがない場合はtiktokenで推定
        assert!(usage.input_tokens.is_some(), "入力トークンが推定されるべき");
        assert!(
            usage.output_tokens.is_some(),
            "出力トークンが推定されるべき"
        );
        // total_tokensは入力+出力
        assert!(usage.total_tokens.is_some());
    }

    #[test]
    fn test_extract_or_estimate_no_text() {
        let response = json!({});

        let usage = extract_or_estimate_tokens(&response, None, None, "gpt-4");

        // テキストもusageもない場合は空
        assert!(usage.is_empty(), "情報がない場合は空のTokenUsageを返すべき");
    }

    #[test]
    fn test_token_usage_is_empty() {
        let empty = TokenUsage::default();
        assert!(empty.is_empty());

        let with_input = TokenUsage::new(Some(10), None, None);
        assert!(!with_input.is_empty());

        let with_output = TokenUsage::new(None, Some(5), None);
        assert!(!with_output.is_empty());

        let with_total = TokenUsage::new(None, None, Some(15));
        assert!(!with_total.is_empty());
    }

    // T-11: SSEチャンクごとのトークン累積テスト
    #[test]
    fn test_streaming_accumulator_parses_chunks() {
        let mut accumulator = StreamingTokenAccumulator::new("gpt-4");

        // SSE形式のチャンク（OpenAI互換）
        let chunk1 = r#"data: {"id":"chatcmpl-123","choices":[{"delta":{"content":"Hello"}}]}"#;
        let chunk2 = r#"data: {"id":"chatcmpl-123","choices":[{"delta":{"content":" world"}}]}"#;
        let chunk3 = r#"data: {"id":"chatcmpl-123","choices":[{"delta":{"content":"!"}}]}"#;

        accumulator.process_chunk(chunk1);
        accumulator.process_chunk(chunk2);
        accumulator.process_chunk(chunk3);

        assert_eq!(accumulator.accumulated_content(), "Hello world!");
    }

    #[test]
    fn test_streaming_accumulator_handles_done_marker() {
        let mut accumulator = StreamingTokenAccumulator::new("gpt-4");

        let chunk1 = r#"data: {"id":"chatcmpl-123","choices":[{"delta":{"content":"Hi"}}]}"#;
        let done = "data: [DONE]";

        accumulator.process_chunk(chunk1);
        accumulator.process_chunk(done);

        assert!(accumulator.is_done());
    }

    #[test]
    fn test_streaming_accumulator_extracts_usage_from_final_chunk() {
        let mut accumulator = StreamingTokenAccumulator::new("gpt-4");

        // 最終チャンクにusageフィールドが含まれるパターン（OpenAI API stream_options.include_usage=true）
        let chunk1 = r#"data: {"id":"chatcmpl-123","choices":[{"delta":{"content":"Test"}}]}"#;
        let final_chunk = r#"data: {"id":"chatcmpl-123","choices":[],"usage":{"prompt_tokens":10,"completion_tokens":5,"total_tokens":15}}"#;
        let done = "data: [DONE]";

        accumulator.process_chunk(chunk1);
        accumulator.process_chunk(final_chunk);
        accumulator.process_chunk(done);

        let usage = accumulator.finalize();
        assert_eq!(usage.input_tokens, Some(10));
        assert_eq!(usage.output_tokens, Some(5));
        assert_eq!(usage.total_tokens, Some(15));
    }

    // T-12: ストリーミング完了時の最終集計テスト
    #[test]
    fn test_streaming_accumulator_estimates_tokens_when_no_usage() {
        let mut accumulator = StreamingTokenAccumulator::new("gpt-4");

        // usageフィールドがないストリーミングレスポンス
        let chunk1 = r#"data: {"id":"chatcmpl-123","choices":[{"delta":{"content":"Hello"}}]}"#;
        let chunk2 = r#"data: {"id":"chatcmpl-123","choices":[{"delta":{"content":" world"}}]}"#;
        let done = "data: [DONE]";

        accumulator.process_chunk(chunk1);
        accumulator.process_chunk(chunk2);
        accumulator.process_chunk(done);

        // usageがない場合はtiktokenで推定
        let usage = accumulator.finalize();
        assert!(
            usage.output_tokens.is_some(),
            "出力トークンが推定されるべき"
        );
        // "Hello world" は2-3トークン程度
        let output = usage.output_tokens.unwrap();
        assert!(output > 0 && output < 10, "妥当なトークン数: {}", output);
    }

    #[test]
    fn test_streaming_accumulator_with_input_tokens() {
        let mut accumulator = StreamingTokenAccumulator::new("gpt-4");
        accumulator.set_input_tokens(Some(25)); // リクエスト時の入力トークン

        let chunk1 = r#"data: {"id":"chatcmpl-123","choices":[{"delta":{"content":"Response"}}]}"#;
        let done = "data: [DONE]";

        accumulator.process_chunk(chunk1);
        accumulator.process_chunk(done);

        let usage = accumulator.finalize();
        assert_eq!(usage.input_tokens, Some(25));
        assert!(usage.output_tokens.is_some());
        // total = input + output
        assert!(usage.total_tokens.is_some());
    }

    #[test]
    fn test_streaming_accumulator_handles_empty_stream() {
        let mut accumulator = StreamingTokenAccumulator::new("gpt-4");

        let done = "data: [DONE]";
        accumulator.process_chunk(done);

        let usage = accumulator.finalize();
        // 空ストリームの場合はすべてNoneまたは0
        assert!(
            usage.is_empty() || usage.output_tokens == Some(0),
            "空ストリームでは空のusageを返すべき"
        );
    }

    #[test]
    fn test_streaming_accumulator_handles_multiline_chunk() {
        let mut accumulator = StreamingTokenAccumulator::new("gpt-4");

        // 複数行を含むチャンク
        let chunk =
            r#"data: {"id":"chatcmpl-123","choices":[{"delta":{"content":"Line1\nLine2"}}]}"#;

        accumulator.process_chunk(chunk);

        assert_eq!(accumulator.accumulated_content(), "Line1\nLine2");
    }

    #[test]
    fn test_streaming_accumulator_collects_responses_api_output_text_delta() {
        let mut accumulator = StreamingTokenAccumulator::new("gpt-4");

        let chunk1 = r#"data: {"type":"response.output_text.delta","delta":"Hello"}"#;
        let chunk2 = r#"data: {"type":"response.output_text.delta","delta":" world"}"#;
        let done = "data: [DONE]";

        accumulator.process_chunk(chunk1);
        accumulator.process_chunk(chunk2);
        accumulator.process_chunk(done);

        assert_eq!(accumulator.accumulated_content(), "Hello world");
        assert!(accumulator.is_done());
    }

    // --- additional coverage tests ---

    #[test]
    fn test_token_usage_new() {
        let usage = TokenUsage::new(Some(10), Some(20), Some(30));
        assert_eq!(usage.input_tokens, Some(10));
        assert_eq!(usage.output_tokens, Some(20));
        assert_eq!(usage.total_tokens, Some(30));
    }

    #[test]
    fn test_token_usage_default_is_empty() {
        let usage = TokenUsage::default();
        assert!(usage.is_empty());
        assert_eq!(usage.input_tokens, None);
        assert_eq!(usage.output_tokens, None);
        assert_eq!(usage.total_tokens, None);
    }

    #[test]
    fn test_token_usage_equality() {
        let a = TokenUsage::new(Some(5), Some(10), Some(15));
        let b = TokenUsage::new(Some(5), Some(10), Some(15));
        assert_eq!(a, b);

        let c = TokenUsage::new(Some(5), Some(11), Some(16));
        assert_ne!(a, c);
    }

    #[test]
    fn test_token_usage_clone() {
        let original = TokenUsage::new(Some(100), Some(200), Some(300));
        let cloned = original.clone();
        assert_eq!(original, cloned);
    }

    #[test]
    fn test_streaming_accumulator_skips_empty_lines() {
        let mut accumulator = StreamingTokenAccumulator::new("gpt-4");

        accumulator.process_chunk("");
        accumulator.process_chunk("   ");
        assert_eq!(accumulator.accumulated_content(), "");
        assert!(!accumulator.is_done());
    }

    #[test]
    fn test_streaming_accumulator_skips_comment_lines() {
        let mut accumulator = StreamingTokenAccumulator::new("gpt-4");

        accumulator.process_chunk(": this is a comment");
        assert_eq!(accumulator.accumulated_content(), "");
    }

    #[test]
    fn test_streaming_accumulator_skips_non_data_lines() {
        let mut accumulator = StreamingTokenAccumulator::new("gpt-4");

        accumulator.process_chunk("event: message");
        accumulator.process_chunk("id: 123");
        assert_eq!(accumulator.accumulated_content(), "");
    }

    #[test]
    fn test_streaming_accumulator_data_colon_no_space() {
        let mut accumulator = StreamingTokenAccumulator::new("gpt-4");

        // "data:" without space after colon
        let chunk = r#"data:{"id":"chatcmpl-123","choices":[{"delta":{"content":"test"}}]}"#;
        accumulator.process_chunk(chunk);
        assert_eq!(accumulator.accumulated_content(), "test");
    }

    #[test]
    fn test_streaming_accumulator_invalid_json_is_silently_ignored() {
        let mut accumulator = StreamingTokenAccumulator::new("gpt-4");

        accumulator.process_chunk("data: not valid json");
        assert_eq!(accumulator.accumulated_content(), "");
    }

    #[test]
    fn test_streaming_accumulator_finalize_with_only_input_tokens() {
        let mut accumulator = StreamingTokenAccumulator::new("gpt-4");
        accumulator.set_input_tokens(Some(50));

        // No content accumulated, no usage extracted
        let done = "data: [DONE]";
        accumulator.process_chunk(done);

        let usage = accumulator.finalize();
        assert_eq!(usage.input_tokens, Some(50));
        assert_eq!(usage.output_tokens, Some(0)); // empty content -> 0
        assert_eq!(usage.total_tokens, Some(50)); // 50 + 0
    }

    #[test]
    fn test_streaming_accumulator_finalize_no_input_no_content() {
        let accumulator = StreamingTokenAccumulator::new("gpt-4");
        let usage = accumulator.finalize();
        assert_eq!(usage.input_tokens, None);
        assert_eq!(usage.output_tokens, Some(0)); // empty content -> 0
        assert_eq!(usage.total_tokens, Some(0));
    }

    #[test]
    fn test_streaming_accumulator_responses_api_output_text_done_fallback() {
        let mut accumulator = StreamingTokenAccumulator::new("gpt-4");

        // response.output_text.done with text, no delta events prior
        let chunk = r#"data: {"type":"response.output_text.done","text":"Full response text"}"#;
        accumulator.process_chunk(chunk);

        assert_eq!(accumulator.accumulated_content(), "Full response text");
    }

    #[test]
    fn test_streaming_accumulator_responses_api_done_ignored_if_delta_accumulated() {
        let mut accumulator = StreamingTokenAccumulator::new("gpt-4");

        // First accumulate via delta
        let delta = r#"data: {"type":"response.output_text.delta","delta":"Hello"}"#;
        accumulator.process_chunk(delta);

        // Then done event with full text should be ignored (delta already accumulated)
        let done = r#"data: {"type":"response.output_text.done","text":"Hello world full text"}"#;
        accumulator.process_chunk(done);

        assert_eq!(accumulator.accumulated_content(), "Hello");
    }

    #[test]
    fn test_streaming_accumulator_unknown_event_type_ignored() {
        let mut accumulator = StreamingTokenAccumulator::new("gpt-4");

        let chunk = r#"data: {"type":"response.created","id":"resp-123"}"#;
        accumulator.process_chunk(chunk);

        assert_eq!(accumulator.accumulated_content(), "");
    }

    #[test]
    fn test_extract_usage_responses_api_nested_usage() {
        // response.done event wraps usage inside "response" key
        let data = json!({
            "type": "response.done",
            "response": {
                "usage": {
                    "input_tokens": 15,
                    "output_tokens": 25,
                    "total_tokens": 40
                }
            }
        });
        let usage = extract_usage_from_response(&data).unwrap();
        assert_eq!(usage.input_tokens, Some(15));
        assert_eq!(usage.output_tokens, Some(25));
        assert_eq!(usage.total_tokens, Some(40));
    }

    #[test]
    fn test_extract_or_estimate_with_only_input_text() {
        let response = json!({});
        let usage = extract_or_estimate_tokens(
            &response,
            Some("This is a test input sentence."),
            None,
            "gpt-4",
        );
        assert!(usage.input_tokens.is_some());
        assert!(usage.output_tokens.is_none());
        // total should equal input when output is None
        assert_eq!(usage.total_tokens, usage.input_tokens);
    }

    #[test]
    fn test_extract_or_estimate_with_only_output_text() {
        let response = json!({});
        let usage = extract_or_estimate_tokens(
            &response,
            None,
            Some("This is a test output sentence."),
            "gpt-4",
        );
        assert!(usage.input_tokens.is_none());
        assert!(usage.output_tokens.is_some());
        assert_eq!(usage.total_tokens, usage.output_tokens);
    }

    #[test]
    fn test_estimate_tokens_long_text() {
        let text = "Hello world. ".repeat(100);
        let tokens = estimate_tokens(&text, "gpt-4");
        assert!(tokens.is_some());
        let count = tokens.unwrap();
        // 1300 chars should produce many tokens
        assert!(count > 50, "expected many tokens for long text: {}", count);
    }

    #[test]
    fn test_estimate_tokens_special_characters() {
        let text = "foo\nbar\ttab \"quoted\" 'single'";
        let tokens = estimate_tokens(text, "gpt-4");
        assert!(tokens.is_some());
        assert!(tokens.unwrap() > 0);
    }

    #[test]
    fn test_streaming_accumulator_multiple_choices_in_single_chunk() {
        let mut accumulator = StreamingTokenAccumulator::new("gpt-4");

        let chunk = r#"data: {"id":"chatcmpl-123","choices":[{"delta":{"content":"A"}},{"delta":{"content":"B"}}]}"#;
        accumulator.process_chunk(chunk);

        assert_eq!(accumulator.accumulated_content(), "AB");
    }

    #[test]
    fn test_streaming_accumulator_usage_overrides_estimation() {
        let mut accumulator = StreamingTokenAccumulator::new("gpt-4");
        accumulator.set_input_tokens(Some(999));

        let chunk = r#"data: {"id":"chatcmpl-123","choices":[{"delta":{"content":"test"}}]}"#;
        let usage_chunk = r#"data: {"id":"chatcmpl-123","choices":[],"usage":{"prompt_tokens":10,"completion_tokens":3,"total_tokens":13}}"#;
        let done = "data: [DONE]";

        accumulator.process_chunk(chunk);
        accumulator.process_chunk(usage_chunk);
        accumulator.process_chunk(done);

        let usage = accumulator.finalize();
        // Should use extracted usage, not estimated
        assert_eq!(usage.input_tokens, Some(10));
        assert_eq!(usage.output_tokens, Some(3));
        assert_eq!(usage.total_tokens, Some(13));
    }
}

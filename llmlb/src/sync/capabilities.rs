//! モデルcapabilities自動判定
//!
//! モデル名プレフィックスと上流申告からcapabilitiesを判定

use crate::types::endpoint::SupportedAPI;

/// モデルが持つ能力
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Capability {
    /// チャット/テキスト生成
    Chat,
    /// 埋め込みベクトル生成
    Embeddings,
    /// 画像入力/視覚理解
    ImageInput,
}

impl Capability {
    /// 文字列表現を取得
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Chat => "chat",
            Self::Embeddings => "embeddings",
            Self::ImageInput => "image_input",
        }
    }
}

impl std::fmt::Display for Capability {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// モデル名からcapabilitiesを自動判定
///
/// # ルール
/// - `embed*` または `*-embed*` → embeddings
/// - それ以外 → chat
///
/// # Examples
///
/// ```
/// use llmlb::sync::capabilities::{detect_capabilities, Capability};
///
/// let caps = detect_capabilities("nomic-embed-text-v1.5");
/// assert_eq!(caps, vec![Capability::Embeddings]);
///
/// let caps = detect_capabilities("llama3.2");
/// assert_eq!(caps, vec![Capability::Chat]);
/// ```
pub fn detect_capabilities(model_name: &str) -> Vec<Capability> {
    let lower = model_name.to_lowercase();
    let leaf = lower.rsplit('/').next().unwrap_or(&lower);

    // embedで始まる、または-embedを含む場合はembeddings
    if leaf.starts_with("embed") || leaf.contains("-embed") || leaf.contains("_embed") {
        return vec![Capability::Embeddings];
    }

    vec![Capability::Chat]
}

/// capabilitiesをJSON用の文字列Vecに変換
pub fn capabilities_to_strings(capabilities: &[Capability]) -> Vec<String> {
    capabilities
        .iter()
        .map(|c| c.as_str().to_string())
        .collect()
}

/// 上流申告の capability 名を llmlb の保存用表現へ正規化する。
pub fn normalize_capability_name(value: &str) -> Option<&'static str> {
    match value
        .trim()
        .to_ascii_lowercase()
        .replace(['-', ' '], "_")
        .as_str()
    {
        "chat" | "chat_completion" | "chat_completions" | "text_generation" => Some("chat"),
        "embedding" | "embeddings" => Some("embeddings"),
        "image_input" | "vision" | "visual" | "multimodal" => Some("image_input"),
        "image_generation" | "images_generation" | "images_generations" => Some("image_generation"),
        "text_to_speech" | "audio_speech" | "tts" => Some("text_to_speech"),
        "speech_to_text" | "audio_transcription" | "audio_transcriptions" | "asr" => {
            Some("speech_to_text")
        }
        _ => None,
    }
}

/// capability 名からDashboard/API表示用の supported_apis を導出する。
pub fn supported_apis_from_capabilities(capabilities: &[String]) -> Vec<SupportedAPI> {
    let mut apis = Vec::new();
    for capability in capabilities {
        let Some(normalized) = normalize_capability_name(capability) else {
            continue;
        };
        let api = match normalized {
            "chat" => Some(SupportedAPI::ChatCompletions),
            "embeddings" => Some(SupportedAPI::Embeddings),
            "image_input" => Some(SupportedAPI::ImageInput),
            "image_generation" => Some(SupportedAPI::ImageGeneration),
            _ => None,
        };
        if let Some(api) = api {
            push_unique_api(&mut apis, api);
        }
    }
    apis.sort_by_key(|api| api.as_str());
    apis
}

/// supported_apis から保存用 capability 名を導出する。
pub fn capability_from_supported_api(api: SupportedAPI) -> Option<&'static str> {
    match api {
        SupportedAPI::ChatCompletions => Some("chat"),
        SupportedAPI::Embeddings => Some("embeddings"),
        SupportedAPI::ImageInput => Some("image_input"),
        SupportedAPI::ImageGeneration => Some("image_generation"),
        SupportedAPI::Responses => None,
    }
}

/// Add a normalized capability name when it is recognized and not already present.
pub fn push_unique_capability(capabilities: &mut Vec<String>, capability: &str) {
    let Some(normalized) = normalize_capability_name(capability) else {
        return;
    };
    if !capabilities.iter().any(|existing| existing == normalized) {
        capabilities.push(normalized.to_string());
    }
}

/// Add a supported API when it is not already present.
pub fn push_unique_api(apis: &mut Vec<SupportedAPI>, api: SupportedAPI) {
    if !apis.contains(&api) {
        apis.push(api);
    }
}

/// 文字列からCapabilityに変換
pub fn capability_from_str(s: &str) -> Option<Capability> {
    match normalize_capability_name(s)? {
        "chat" => Some(Capability::Chat),
        "embeddings" => Some(Capability::Embeddings),
        "image_input" => Some(Capability::ImageInput),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_capabilities_embed_prefix() {
        // embedで始まるモデル
        assert_eq!(
            detect_capabilities("embed-text-v1"),
            vec![Capability::Embeddings]
        );
        assert_eq!(
            detect_capabilities("EMBED-multilingual"),
            vec![Capability::Embeddings]
        );
    }

    #[test]
    fn test_detect_capabilities_embed_suffix() {
        // -embedを含むモデル
        assert_eq!(
            detect_capabilities("nomic-embed-text-v1.5"),
            vec![Capability::Embeddings]
        );
        assert_eq!(
            detect_capabilities("bge-embed-large"),
            vec![Capability::Embeddings]
        );
        assert_eq!(
            detect_capabilities("model_embed_v2"),
            vec![Capability::Embeddings]
        );
    }

    #[test]
    fn test_detect_capabilities_chat() {
        // 通常のチャットモデル
        assert_eq!(detect_capabilities("llama3.2"), vec![Capability::Chat]);
        assert_eq!(detect_capabilities("gpt-4"), vec![Capability::Chat]);
        assert_eq!(
            detect_capabilities("gemma-2b-instruct"),
            vec![Capability::Chat]
        );
        assert_eq!(detect_capabilities("qwen2.5"), vec![Capability::Chat]);
    }

    #[test]
    fn test_detect_capabilities_case_insensitive() {
        // 大文字小文字を区別しない
        assert_eq!(
            detect_capabilities("NOMIC-EMBED-TEXT"),
            vec![Capability::Embeddings]
        );
        assert_eq!(
            detect_capabilities("Embed-Model"),
            vec![Capability::Embeddings]
        );
    }

    #[test]
    fn test_capabilities_to_strings() {
        let caps = vec![Capability::Chat];
        assert_eq!(capabilities_to_strings(&caps), vec!["chat".to_string()]);

        let caps = vec![Capability::Embeddings];
        assert_eq!(
            capabilities_to_strings(&caps),
            vec!["embeddings".to_string()]
        );

        let caps = vec![Capability::ImageInput];
        assert_eq!(
            capabilities_to_strings(&caps),
            vec!["image_input".to_string()]
        );
    }

    #[test]
    fn test_capability_from_str() {
        assert_eq!(capability_from_str("chat"), Some(Capability::Chat));
        assert_eq!(capability_from_str("CHAT"), Some(Capability::Chat));
        assert_eq!(
            capability_from_str("embeddings"),
            Some(Capability::Embeddings)
        );
        assert_eq!(capability_from_str("vision"), Some(Capability::ImageInput));
        assert_eq!(capability_from_str("unknown"), None);
    }

    #[test]
    fn test_supported_apis_from_capabilities_maps_vision_to_image_input() {
        let apis = supported_apis_from_capabilities(&["chat".to_string(), "vision".to_string()]);

        assert_eq!(
            apis,
            vec![SupportedAPI::ChatCompletions, SupportedAPI::ImageInput]
        );
    }

    #[test]
    fn test_capability_as_str() {
        assert_eq!(Capability::Chat.as_str(), "chat");
        assert_eq!(Capability::Embeddings.as_str(), "embeddings");
    }

    #[test]
    fn test_detect_capabilities_default_chat_models() {
        // Regular LLM should only have chat capability
        let caps = detect_capabilities("llama-3.1-8b");
        assert!(caps.contains(&Capability::Chat));

        let caps = detect_capabilities("mistral-7b");
        assert!(caps.contains(&Capability::Chat));
    }
}

//! Built-in canonical-to-engine model mappings.
//!
//! The canonical identifier is always the Hugging Face repo ID. The aliases in
//! this file describe the engine-specific runtime names that llmlb knows how to
//! translate to and from.
//!
//! This table is llmlb's source of truth for built-in support across
//! engine-specific runtimes. A model can be absent from every endpoint's
//! current `/v1/models` inventory and still be considered supported when it
//! appears here.
//!
//! Inventory and support are intentionally separate:
//! - endpoint sync stores `canonical_name` only for runtime model IDs that
//!   resolve through this table
//! - `/v1/models` returns only models currently reported by online endpoints
//! - the `/v1/models` response prefers `canonical_name` for the returned `id`

use crate::types::endpoint::EndpointType;

/// Engine-specific runtime model name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineAlias {
    /// Endpoint type that reports or accepts this alias.
    pub engine: EndpointType,
    /// Runtime model identifier used by that endpoint type.
    pub name: &'static str,
}

/// Canonical model mapping entry.
#[derive(Debug, Clone)]
pub struct ModelMapping {
    /// Canonical Hugging Face repo ID.
    pub canonical: &'static str,
    /// Known runtime aliases for supported endpoint types.
    pub aliases: &'static [EngineAlias],
}

fn model_id_eq(left: &str, right: &str) -> bool {
    left == right || left.eq_ignore_ascii_case(right)
}

/// Built-in compatibility table keyed by canonical Hugging Face repo ID.
pub static BUILTIN_MAPPINGS: &[ModelMapping] = &[
    ModelMapping {
        canonical: "openai/gpt-oss-20b",
        aliases: &[
            EngineAlias {
                engine: EndpointType::Ollama,
                name: "gpt-oss:20b",
            },
            EngineAlias {
                engine: EndpointType::LmStudio,
                name: "openai/gpt-oss-20b",
            },
        ],
    },
    ModelMapping {
        canonical: "openai/gpt-oss-120b",
        aliases: &[
            EngineAlias {
                engine: EndpointType::Ollama,
                name: "gpt-oss:120b",
            },
            EngineAlias {
                engine: EndpointType::LmStudio,
                name: "openai/gpt-oss-120b",
            },
        ],
    },
    ModelMapping {
        canonical: "Qwen/Qwen3-Coder-30B-A3B-Instruct",
        aliases: &[
            EngineAlias {
                engine: EndpointType::Ollama,
                name: "qwen3-coder:30b",
            },
            EngineAlias {
                engine: EndpointType::Ollama,
                name: "qwen3-coder:latest",
            },
            EngineAlias {
                engine: EndpointType::LmStudio,
                name: "qwen/qwen3-coder-30b",
            },
        ],
    },
    ModelMapping {
        canonical: "Qwen/Qwen3-30B",
        aliases: &[
            EngineAlias {
                engine: EndpointType::Ollama,
                name: "qwen3:30b",
            },
            EngineAlias {
                engine: EndpointType::LmStudio,
                name: "qwen/qwen3-30b-a3b",
            },
        ],
    },
    ModelMapping {
        canonical: "Qwen/Qwen3-Coder-Next",
        aliases: &[
            EngineAlias {
                engine: EndpointType::Ollama,
                name: "qwen3-coder-next:latest",
            },
            EngineAlias {
                engine: EndpointType::Ollama,
                name: "qwen3-coder-next",
            },
            EngineAlias {
                engine: EndpointType::LmStudio,
                name: "qwen/qwen3-coder-next",
            },
        ],
    },
    ModelMapping {
        canonical: "meta-llama/Llama-3.3-70B-Instruct",
        aliases: &[
            EngineAlias {
                engine: EndpointType::Ollama,
                name: "llama3.3:70b",
            },
            EngineAlias {
                engine: EndpointType::LmStudio,
                name: "meta/llama-3.3-70b",
            },
        ],
    },
    ModelMapping {
        canonical: "google/gemma-3-27b-it",
        aliases: &[
            EngineAlias {
                engine: EndpointType::Ollama,
                name: "gemma3:27b",
            },
            EngineAlias {
                engine: EndpointType::LmStudio,
                name: "google/gemma-3-27b",
            },
        ],
    },
    ModelMapping {
        canonical: "Qwen/Qwen3.5-35B-A3B",
        aliases: &[
            EngineAlias {
                engine: EndpointType::Ollama,
                name: "qwen3.5:35b-a3b",
            },
            EngineAlias {
                engine: EndpointType::Ollama,
                name: "qwen3.5-35b-a3b",
            },
            EngineAlias {
                engine: EndpointType::Ollama,
                name: "qwen3.5:latest",
            },
            EngineAlias {
                engine: EndpointType::LmStudio,
                name: "qwen3.5-35b-a3b",
            },
            EngineAlias {
                engine: EndpointType::LmStudio,
                name: "qwen/qwen3.5-35b-a3b",
            },
            EngineAlias {
                engine: EndpointType::LmStudio,
                name: "qwen/qwen3.5-35b-a3b:2",
            },
        ],
    },
    ModelMapping {
        canonical: "nvidia/nemotron-3-super-120b-a12b",
        aliases: &[
            EngineAlias {
                engine: EndpointType::Ollama,
                name: "nemotron-3-super:120b-a12b",
            },
            EngineAlias {
                engine: EndpointType::LmStudio,
                name: "nvidia-nemotron-3-super-120b-a12b",
            },
            EngineAlias {
                engine: EndpointType::LmStudio,
                name: "nvidia/nemotron-3-super",
            },
            EngineAlias {
                engine: EndpointType::LmStudio,
                name: "unsloth/nvidia-nemotron-3-super-120b-a12b",
            },
        ],
    },
    ModelMapping {
        canonical: "nvidia/Nemotron-3-Nano",
        aliases: &[
            EngineAlias {
                engine: EndpointType::Ollama,
                name: "nemotron-3-nano:30b",
            },
            EngineAlias {
                engine: EndpointType::LmStudio,
                name: "nvidia/nemotron-3-nano",
            },
            EngineAlias {
                engine: EndpointType::LmStudio,
                name: "nvidia/nemotron-3-nano-4b",
            },
        ],
    },
    ModelMapping {
        canonical: "Qwen/Qwen2.5-14B-Instruct-AWQ",
        aliases: &[
            EngineAlias {
                engine: EndpointType::Ollama,
                name: "qwen2.5:14b-instruct",
            },
            EngineAlias {
                engine: EndpointType::LmStudio,
                name: "Qwen/Qwen2.5-14B-Instruct-AWQ",
            },
        ],
    },
    ModelMapping {
        canonical: "nomic-ai/nomic-embed-text-v1.5",
        aliases: &[
            EngineAlias {
                engine: EndpointType::Ollama,
                name: "nomic-embed-text:latest",
            },
            EngineAlias {
                engine: EndpointType::Ollama,
                name: "nomic-embed-text",
            },
            EngineAlias {
                engine: EndpointType::Ollama,
                name: "nomic-embed-text:v1.5",
            },
            EngineAlias {
                engine: EndpointType::LmStudio,
                name: "text-embedding-nomic-embed-text-v1.5",
            },
            EngineAlias {
                engine: EndpointType::LmStudio,
                name: "nomic-ai/nomic-embed-text-v1.5",
            },
        ],
    },
    // GLM-4.7-Flash: HuggingFace 上の現行リポジトリは `zai-org/glm-4.7-flash`（旧 THUDM）。
    // canonical は実在するリポジトリ ID に合わせ、`THUDM/...` は alias として残す。
    ModelMapping {
        canonical: "zai-org/glm-4.7-flash",
        aliases: &[
            EngineAlias {
                engine: EndpointType::Ollama,
                name: "glm-4.7-flash:latest",
            },
            EngineAlias {
                engine: EndpointType::Ollama,
                name: "glm-4.7-flash",
            },
            EngineAlias {
                engine: EndpointType::LmStudio,
                name: "THUDM/glm-4.7-flash",
            },
        ],
    },
    // Gemma 4 (26B-A4B): `:latest` は将来世代の登場で意味がねじれる反パターンのため alias から外す。
    // 具体タグ `gemma4` のみを Ollama alias として保持。
    ModelMapping {
        canonical: "google/gemma-4-26b-a4b",
        aliases: &[
            EngineAlias {
                engine: EndpointType::Ollama,
                name: "gemma4",
            },
            EngineAlias {
                engine: EndpointType::LmStudio,
                name: "google/gemma-4-26b-a4b",
            },
        ],
    },
];

/// Resolve a runtime model ID to its canonical Hugging Face repo ID.
pub fn resolve_canonical(model_id: &str, endpoint_type: &EndpointType) -> Option<&'static str> {
    for mapping in BUILTIN_MAPPINGS {
        if model_id_eq(mapping.canonical, model_id) {
            return Some(mapping.canonical);
        }

        for alias in mapping.aliases {
            if alias.engine == *endpoint_type && model_id_eq(alias.name, model_id) {
                return Some(mapping.canonical);
            }
        }
    }

    None
}

/// Resolve the first engine-specific alias for a canonical model.
pub fn resolve_engine_name(canonical: &str, endpoint_type: &EndpointType) -> Option<&'static str> {
    resolve_engine_names(canonical, endpoint_type)
        .into_iter()
        .next()
}

/// Resolve all engine-specific aliases for a canonical model.
/// Supports both canonical IDs and legacy aliases for backward compatibility.
pub fn resolve_engine_names(canonical: &str, endpoint_type: &EndpointType) -> Vec<&'static str> {
    // find_mapping accepts both canonical IDs and aliases, enabling backward compatibility
    // with legacy canonical IDs that may have been used in external requests.
    if let Some(mapping) = find_mapping(canonical) {
        return mapping
            .aliases
            .iter()
            .filter(|alias| alias.engine == *endpoint_type)
            .map(|alias| alias.name)
            .collect();
    }

    Vec::new()
}

/// Returns whether llmlb has a built-in mapping for this canonical model on the given endpoint type.
pub fn supports_canonical_on_endpoint(canonical: &str, endpoint_type: &EndpointType) -> bool {
    !resolve_engine_names(canonical, endpoint_type).is_empty()
}

/// Find the built-in mapping by canonical ID or by any known alias.
pub fn find_mapping(model_id: &str) -> Option<&'static ModelMapping> {
    for mapping in BUILTIN_MAPPINGS {
        if model_id_eq(mapping.canonical, model_id) {
            return Some(mapping);
        }

        for alias in mapping.aliases {
            if model_id_eq(alias.name, model_id) {
                return Some(mapping);
            }
        }
    }

    None
}

/// Best-effort fallback from an engine model ID to a likely HF repo ID.
pub fn guess_hf_repo(model_id: &str, endpoint_type: &EndpointType) -> Option<String> {
    match endpoint_type {
        EndpointType::LmStudio => {
            if model_id.contains('/') && !model_id.contains(':') {
                Some(model_id.to_string())
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Canonical name resolution result built from a set of endpoint models.
///
/// Used by both `/v1/models` and the dashboard API to merge models that share
/// the same canonical name (HuggingFace repo ID) into a single entry.
#[derive(Debug, Default)]
pub struct CanonicalResolution {
    /// canonical_name → engine-specific aliases that differ from it.
    pub canonical_to_aliases: std::collections::HashMap<String, std::collections::HashSet<String>>,
    /// engine model_id → canonical_name (reverse lookup).
    pub model_to_canonical: std::collections::HashMap<String, String>,
}

impl CanonicalResolution {
    /// Resolve the canonical name to display for a given model key.
    ///
    /// 解決順:
    /// 1. `model_key` 自体が canonical 名（`canonical_to_aliases` にエントリあり）→ そのまま返す。
    /// 2. `model_to_canonical` の逆引きで canonical が見つかる → それを返す。
    /// 3. いずれも該当なし → `model_key` 自身を canonical として返す（self-canonical fallback）。
    ///
    /// fallback により `/v1/models` レスポンスで `canonical_name: null` が出ることがなくなる。
    /// 「mapping に登録があるか？」を判定したい場合は [`Self::is_known`] を使用する。
    pub fn canonical_for(&self, model_key: &str) -> String {
        if self.canonical_to_aliases.contains_key(model_key) {
            model_key.to_string()
        } else if let Some(canonical) = self.model_to_canonical.get(model_key) {
            canonical.clone()
        } else {
            model_key.to_string()
        }
    }

    /// `model_key` が canonical テーブルに登録されているか。
    pub fn is_known(&self, model_key: &str) -> bool {
        self.canonical_to_aliases.contains_key(model_key)
            || self.model_to_canonical.contains_key(model_key)
    }

    /// Sorted aliases for a given model key.
    pub fn aliases_for(&self, model_key: &str) -> Vec<String> {
        self.canonical_to_aliases
            .get(model_key)
            .map(|a| {
                let mut v: Vec<String> = a.iter().cloned().collect();
                v.sort();
                v
            })
            .unwrap_or_default()
    }
}

/// Build a [`CanonicalResolution`] from an iterator of `(model_id, canonical_name)` pairs.
pub fn build_canonical_maps<'a>(
    models: impl Iterator<Item = (&'a str, Option<&'a str>)>,
) -> CanonicalResolution {
    let mut res = CanonicalResolution::default();
    for (model_id, canonical) in models {
        let Some(canonical) = canonical else {
            continue;
        };
        if canonical != model_id {
            res.canonical_to_aliases
                .entry(canonical.to_string())
                .or_default()
                .insert(model_id.to_string());
        }
        res.model_to_canonical
            .insert(model_id.to_string(), canonical.to_string());
    }
    res
}

/// 既知 canonical モデルのコンテキスト長フォールバックテーブル。
///
/// エンドポイントが `/v1/models` で `max_tokens` を申告しないケース（B-1, G-7）への対策として、
/// 公開情報から確認できる代表的なモデルの context window をハードコードしている。
/// エンドポイント申告が優先されるため、このテーブルはあくまで欠損値補填用。
///
/// 値の出典: 各モデルの HuggingFace モデルカード／公式リリースノート時点での公称 context length。
const KNOWN_CONTEXT_LENGTHS: &[(&str, u32)] = &[
    // OpenAI gpt-oss
    ("openai/gpt-oss-20b", 131_072),
    ("openai/gpt-oss-120b", 131_072),
    // Qwen
    ("Qwen/Qwen3-Coder-30B-A3B-Instruct", 262_144),
    ("Qwen/Qwen3.5-35B-A3B", 262_144),
    ("Qwen/Qwen2.5-14B-Instruct-AWQ", 32_768),
    // Google Gemma
    ("google/gemma-3-27b-it", 131_072),
    ("google/gemma-4-26b-a4b", 131_072),
    // GLM
    ("zai-org/glm-4.7-flash", 131_072),
    // Nvidia Nemotron
    ("nvidia/nemotron-3-super-120b-a12b", 131_072),
    ("nvidia/Nemotron-3-Nano", 131_072),
    // Meta Llama
    ("meta-llama/Llama-3.3-70B-Instruct", 131_072),
    // Embeddings
    ("nomic-ai/nomic-embed-text-v1.5", 8_192),
];

/// 既知 canonical の context length を返す（未登録なら `None`）。
pub fn known_max_tokens(canonical: &str) -> Option<u32> {
    KNOWN_CONTEXT_LENGTHS
        .iter()
        .find(|(name, _)| *name == canonical)
        .map(|(_, len)| *len)
}

/// `max_tokens` をエンドポイント申告 → known テーブルの順に解決する。
///
/// 値の優先順位:
/// 1. エンドポイントが申告した値（`endpoint_reported`）。
/// 2. `KNOWN_CONTEXT_LENGTHS` のフォールバック値。
/// 3. いずれも該当なし → `None`（API レスポンスでは `null` として表現）。
pub fn resolve_max_tokens(canonical: &str, endpoint_reported: Option<u32>) -> Option<u32> {
    endpoint_reported.or_else(|| known_max_tokens(canonical))
}

/// Resolve a canonical ID by matching against any known alias regardless of endpoint type.
pub fn resolve_canonical_any(model_id: &str) -> Option<&'static str> {
    for mapping in BUILTIN_MAPPINGS {
        if model_id_eq(mapping.canonical, model_id) {
            return Some(mapping.canonical);
        }

        for alias in mapping.aliases {
            if model_id_eq(alias.name, model_id) {
                return Some(mapping.canonical);
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_canonical_by_ollama_name() {
        let result = resolve_canonical("gpt-oss:20b", &EndpointType::Ollama);
        assert_eq!(result, Some("openai/gpt-oss-20b"));
    }

    #[test]
    fn test_resolve_canonical_by_lm_studio_name() {
        let result = resolve_canonical("openai/gpt-oss-20b", &EndpointType::LmStudio);
        assert_eq!(result, Some("openai/gpt-oss-20b"));
    }

    #[test]
    fn test_resolve_canonical_by_canonical_name() {
        let result = resolve_canonical("openai/gpt-oss-20b", &EndpointType::Ollama);
        assert_eq!(result, Some("openai/gpt-oss-20b"));
    }

    #[test]
    fn test_resolve_canonical_unknown() {
        let result = resolve_canonical("unknown-model", &EndpointType::Ollama);
        assert!(result.is_none());
    }

    #[test]
    fn test_resolve_canonical_wrong_engine() {
        let result = resolve_canonical("gpt-oss:20b", &EndpointType::Vllm);
        assert!(result.is_none());
    }

    #[test]
    fn test_resolve_engine_name_ollama() {
        let result = resolve_engine_name("openai/gpt-oss-20b", &EndpointType::Ollama);
        assert_eq!(result, Some("gpt-oss:20b"));
    }

    #[test]
    fn test_resolve_engine_name_lm_studio() {
        let result = resolve_engine_name("openai/gpt-oss-20b", &EndpointType::LmStudio);
        assert_eq!(result, Some("openai/gpt-oss-20b"));
    }

    #[test]
    fn test_resolve_engine_name_no_alias() {
        let result = resolve_engine_name("openai/gpt-oss-20b", &EndpointType::Vllm);
        assert!(result.is_none());
    }

    #[test]
    fn test_resolve_engine_name_unknown_canonical() {
        let result = resolve_engine_name("unknown/model", &EndpointType::Ollama);
        assert!(result.is_none());
    }

    #[test]
    fn test_resolve_engine_names_lm_studio_returns_all_aliases() {
        let result = resolve_engine_names("Qwen/Qwen3.5-35B-A3B", &EndpointType::LmStudio);
        assert_eq!(
            result,
            vec![
                "qwen3.5-35b-a3b",
                "qwen/qwen3.5-35b-a3b",
                "qwen/qwen3.5-35b-a3b:2"
            ]
        );
    }

    #[test]
    fn test_resolve_engine_names_unknown_canonical_returns_empty() {
        let result = resolve_engine_names("unknown/model", &EndpointType::LmStudio);
        assert!(result.is_empty());
    }

    #[test]
    fn test_supports_canonical_on_endpoint_true_when_alias_exists() {
        assert!(supports_canonical_on_endpoint(
            "openai/gpt-oss-20b",
            &EndpointType::Ollama
        ));
        assert!(supports_canonical_on_endpoint(
            "openai/gpt-oss-20b",
            &EndpointType::LmStudio
        ));
    }

    #[test]
    fn test_supports_canonical_on_endpoint_false_when_alias_missing() {
        assert!(!supports_canonical_on_endpoint(
            "openai/gpt-oss-20b",
            &EndpointType::Vllm
        ));
        assert!(!supports_canonical_on_endpoint(
            "unknown/model",
            &EndpointType::Ollama
        ));
    }

    #[test]
    fn test_find_mapping_by_canonical() {
        let mapping = find_mapping("openai/gpt-oss-20b");
        assert!(mapping.is_some());
        let m = mapping.unwrap();
        assert_eq!(m.canonical, "openai/gpt-oss-20b");
        assert!(!m.aliases.is_empty());
    }

    #[test]
    fn test_find_mapping_by_alias() {
        let mapping = find_mapping("gpt-oss:20b");
        assert!(mapping.is_some());
        assert_eq!(mapping.unwrap().canonical, "openai/gpt-oss-20b");
    }

    #[test]
    fn test_find_mapping_not_found() {
        let mapping = find_mapping("nonexistent-model");
        assert!(mapping.is_none());
    }

    #[test]
    fn test_guess_hf_repo_lm_studio() {
        let result = guess_hf_repo(
            "lmstudio-community/gemma-3-1b-it-GGUF",
            &EndpointType::LmStudio,
        );
        assert_eq!(
            result,
            Some("lmstudio-community/gemma-3-1b-it-GGUF".to_string())
        );
    }

    #[test]
    fn test_guess_hf_repo_lm_studio_no_slash() {
        let result = guess_hf_repo("gemma-3-1b", &EndpointType::LmStudio);
        assert!(result.is_none());
    }

    #[test]
    fn test_guess_hf_repo_ollama() {
        let result = guess_hf_repo("gemma3:27b", &EndpointType::Ollama);
        assert!(result.is_none());
    }

    #[test]
    fn test_resolve_canonical_any_by_canonical() {
        let result = resolve_canonical_any("openai/gpt-oss-20b");
        assert_eq!(result, Some("openai/gpt-oss-20b"));
    }

    #[test]
    fn test_resolve_canonical_any_by_alias() {
        let result = resolve_canonical_any("gpt-oss:20b");
        assert_eq!(result, Some("openai/gpt-oss-20b"));
    }

    #[test]
    fn test_resolve_canonical_any_unknown() {
        let result = resolve_canonical_any("unknown-model");
        assert!(result.is_none());
    }

    #[test]
    fn test_builtin_mappings_not_empty() {
        assert!(!BUILTIN_MAPPINGS.is_empty());
    }

    #[test]
    fn test_all_mappings_have_aliases() {
        for mapping in BUILTIN_MAPPINGS {
            assert!(
                !mapping.aliases.is_empty(),
                "Mapping for {} has no aliases",
                mapping.canonical
            );
        }
    }

    #[test]
    fn test_qwen3_coder_mapping() {
        let result = resolve_canonical("qwen3-coder:30b", &EndpointType::Ollama);
        assert_eq!(result, Some("Qwen/Qwen3-Coder-30B-A3B-Instruct"));
    }

    #[test]
    fn test_qwen3_coder_lm_studio_lowercase_mapping() {
        let result = resolve_canonical("qwen/qwen3-coder-30b", &EndpointType::LmStudio);
        assert_eq!(result, Some("Qwen/Qwen3-Coder-30B-A3B-Instruct"));
    }

    #[test]
    fn test_qwen3_coder_latest_mapping() {
        let result = resolve_canonical("qwen3-coder:latest", &EndpointType::Ollama);
        assert_eq!(result, Some("Qwen/Qwen3-Coder-30B-A3B-Instruct"));
    }

    #[test]
    fn test_qwen3_coder_next_mapping() {
        let result = resolve_canonical("qwen/qwen3-coder-next", &EndpointType::LmStudio);
        assert_eq!(result, Some("Qwen/Qwen3-Coder-Next"));
    }

    #[test]
    fn test_qwen35_mapping() {
        let result = resolve_canonical("qwen3.5:latest", &EndpointType::Ollama);
        assert_eq!(result, Some("Qwen/Qwen3.5-35B-A3B"));
    }

    #[test]
    fn test_glm47_mapping() {
        // canonical は zai-org に統一（HF 上の現行リポジトリ）
        let result = resolve_canonical("zai-org/glm-4.7-flash", &EndpointType::LmStudio);
        assert_eq!(result, Some("zai-org/glm-4.7-flash"));

        // 旧 THUDM 名は alias として canonical に解決される
        let legacy = resolve_canonical("THUDM/glm-4.7-flash", &EndpointType::LmStudio);
        assert_eq!(legacy, Some("zai-org/glm-4.7-flash"));
    }

    #[test]
    fn test_nomic_embedding_mapping() {
        let result = resolve_canonical(
            "text-embedding-nomic-embed-text-v1.5",
            &EndpointType::LmStudio,
        );
        assert_eq!(result, Some("nomic-ai/nomic-embed-text-v1.5"));
    }

    #[test]
    fn test_nemotron_super_unsloth_mapping() {
        let result = resolve_canonical(
            "unsloth/nvidia-nemotron-3-super-120b-a12b",
            &EndpointType::LmStudio,
        );
        assert_eq!(result, Some("nvidia/nemotron-3-super-120b-a12b"));
    }

    #[test]
    fn test_gemma3_mapping() {
        let result = resolve_canonical("gemma3:27b", &EndpointType::Ollama);
        assert_eq!(result, Some("google/gemma-3-27b-it"));
    }

    #[test]
    fn test_llama33_mapping() {
        let result = resolve_canonical("llama3.3:70b", &EndpointType::Ollama);
        assert_eq!(result, Some("meta-llama/Llama-3.3-70B-Instruct"));
    }

    #[test]
    fn test_nvidia_nemotron_super_mapping() {
        let result = resolve_canonical("nemotron-3-super:120b-a12b", &EndpointType::Ollama);
        assert_eq!(result, Some("nvidia/nemotron-3-super-120b-a12b"));
    }

    #[test]
    fn test_nvidia_nemotron_nano_mapping() {
        let ollama = resolve_canonical("nemotron-3-nano:30b", &EndpointType::Ollama);
        assert_eq!(ollama, Some("nvidia/Nemotron-3-Nano"));

        let result = resolve_canonical("nvidia/nemotron-3-nano", &EndpointType::LmStudio);
        assert_eq!(result, Some("nvidia/Nemotron-3-Nano"));
    }

    #[test]
    fn test_nomic_embed_mapping() {
        let ollama = resolve_canonical("nomic-embed-text:latest", &EndpointType::Ollama);
        assert_eq!(ollama, Some("nomic-ai/nomic-embed-text-v1.5"));

        let result = resolve_canonical(
            "text-embedding-nomic-embed-text-v1.5",
            &EndpointType::LmStudio,
        );
        assert_eq!(result, Some("nomic-ai/nomic-embed-text-v1.5"));
    }

    #[test]
    fn test_glm_flash_mapping() {
        let ollama = resolve_canonical("glm-4.7-flash:latest", &EndpointType::Ollama);
        assert_eq!(ollama, Some("zai-org/glm-4.7-flash"));

        let result = resolve_canonical("zai-org/glm-4.7-flash", &EndpointType::LmStudio);
        assert_eq!(result, Some("zai-org/glm-4.7-flash"));

        // legacy THUDM/... も alias 経由で canonical に解決される
        let legacy = resolve_canonical("THUDM/glm-4.7-flash", &EndpointType::LmStudio);
        assert_eq!(legacy, Some("zai-org/glm-4.7-flash"));
    }

    #[test]
    fn test_qwen25_awq_mapping() {
        let ollama = resolve_canonical("qwen2.5:14b-instruct", &EndpointType::Ollama);
        assert_eq!(ollama, Some("Qwen/Qwen2.5-14B-Instruct-AWQ"));

        let result = resolve_canonical("Qwen/Qwen2.5-14B-Instruct-AWQ", &EndpointType::LmStudio);
        assert_eq!(result, Some("Qwen/Qwen2.5-14B-Instruct-AWQ"));
    }

    #[test]
    fn test_qwen35_all_variants_resolve_to_same_canonical() {
        let ollama = resolve_canonical("qwen3.5:35b-a3b", &EndpointType::Ollama);
        let ollama_legacy = resolve_canonical("qwen3.5-35b-a3b", &EndpointType::Ollama);
        let lms_short = resolve_canonical("qwen3.5-35b-a3b", &EndpointType::LmStudio);
        let lms = resolve_canonical("qwen/qwen3.5-35b-a3b", &EndpointType::LmStudio);
        let lms_v2 = resolve_canonical("qwen/qwen3.5-35b-a3b:2", &EndpointType::LmStudio);
        assert_eq!(ollama, ollama_legacy);
        assert_eq!(ollama, lms_short);
        assert_eq!(lms_short, lms);
        assert_eq!(lms, lms_v2);
        assert_eq!(ollama, Some("Qwen/Qwen3.5-35B-A3B"));
    }

    #[test]
    fn test_gemma4_ollama_resolves_to_canonical() {
        // `gemma4:latest` は撤廃済み（将来世代の登場で意味がねじれるため）。
        let removed = resolve_canonical("gemma4:latest", &EndpointType::Ollama);
        assert_eq!(removed, None);

        // 具体タグ `gemma4` は引き続き alias として解決される。
        let result = resolve_canonical("gemma4", &EndpointType::Ollama);
        assert_eq!(result, Some("google/gemma-4-26b-a4b"));
    }

    #[test]
    fn test_gemma4_lm_studio_resolves_to_canonical() {
        let result = resolve_canonical("google/gemma-4-26b-a4b", &EndpointType::LmStudio);
        assert_eq!(result, Some("google/gemma-4-26b-a4b"));
    }

    #[test]
    fn test_gemma4_engine_name_resolution() {
        // `:latest` 撤廃により Ollama 側の優先 alias は具体タグ `gemma4`
        let ollama = resolve_engine_name("google/gemma-4-26b-a4b", &EndpointType::Ollama);
        assert_eq!(ollama, Some("gemma4"));

        let lms = resolve_engine_name("google/gemma-4-26b-a4b", &EndpointType::LmStudio);
        assert_eq!(lms, Some("google/gemma-4-26b-a4b"));
    }

    #[test]
    fn test_nemotron_nano_4b_alias_resolves() {
        let result = resolve_canonical("nvidia/nemotron-3-nano-4b", &EndpointType::LmStudio);
        assert_eq!(result, Some("nvidia/Nemotron-3-Nano"));
    }

    #[test]
    fn test_recently_added_lm_studio_aliases_resolve() {
        let cases = [
            ("openai/gpt-oss-120b", "openai/gpt-oss-120b"),
            ("Qwen/Qwen3-Coder-30B-A3B-Instruct", "qwen/qwen3-coder-30b"),
            ("Qwen/Qwen3-30B", "qwen/qwen3-30b-a3b"),
            ("meta-llama/Llama-3.3-70B-Instruct", "meta/llama-3.3-70b"),
            ("google/gemma-3-27b-it", "google/gemma-3-27b"),
            (
                "nvidia/nemotron-3-super-120b-a12b",
                "nvidia-nemotron-3-super-120b-a12b",
            ),
        ];

        for (canonical, alias) in cases {
            let result = resolve_canonical(alias, &EndpointType::LmStudio);
            assert_eq!(result, Some(canonical), "failed for {}", alias);
        }
    }

    #[test]
    fn test_build_canonical_maps_merges_aliases() {
        let models = vec![
            ("gpt-oss:20b", Some("openai/gpt-oss-20b")),
            ("openai/gpt-oss-20b", Some("openai/gpt-oss-20b")),
            ("qwen3.5:35b-a3b", Some("Qwen/Qwen3.5-35B-A3B")),
            ("qwen/qwen3.5-35b-a3b", Some("Qwen/Qwen3.5-35B-A3B")),
            ("unknown-model", None),
        ];
        let res = build_canonical_maps(models.into_iter());

        assert_eq!(res.canonical_to_aliases.len(), 2);
        assert!(res
            .canonical_to_aliases
            .get("openai/gpt-oss-20b")
            .unwrap()
            .contains("gpt-oss:20b"));
        assert!(res
            .canonical_to_aliases
            .get("Qwen/Qwen3.5-35B-A3B")
            .unwrap()
            .contains("qwen3.5:35b-a3b"));
        assert!(res
            .canonical_to_aliases
            .get("Qwen/Qwen3.5-35B-A3B")
            .unwrap()
            .contains("qwen/qwen3.5-35b-a3b"));
    }

    #[test]
    fn test_canonical_for_returns_canonical_when_key_is_canonical() {
        let models = vec![
            ("gpt-oss:20b", Some("openai/gpt-oss-20b")),
            ("openai/gpt-oss-20b", Some("openai/gpt-oss-20b")),
        ];
        let res = build_canonical_maps(models.into_iter());

        assert_eq!(
            res.canonical_for("openai/gpt-oss-20b"),
            "openai/gpt-oss-20b"
        );
    }

    #[test]
    fn test_canonical_for_returns_canonical_when_key_is_alias() {
        let models = vec![("gpt-oss:20b", Some("openai/gpt-oss-20b"))];
        let res = build_canonical_maps(models.into_iter());

        assert_eq!(res.canonical_for("gpt-oss:20b"), "openai/gpt-oss-20b");
    }

    #[test]
    fn test_known_max_tokens_returns_value_for_known_canonical() {
        assert_eq!(known_max_tokens("openai/gpt-oss-20b"), Some(131_072));
        assert_eq!(
            known_max_tokens("Qwen/Qwen3-Coder-30B-A3B-Instruct"),
            Some(262_144)
        );
        assert_eq!(known_max_tokens("zai-org/glm-4.7-flash"), Some(131_072));
        assert_eq!(
            known_max_tokens("nomic-ai/nomic-embed-text-v1.5"),
            Some(8_192)
        );
    }

    #[test]
    fn test_known_max_tokens_returns_none_for_unknown() {
        assert_eq!(known_max_tokens("ggml-org/gemma-4-E2B-it-GGUF"), None);
        assert_eq!(known_max_tokens(""), None);
    }

    #[test]
    fn test_resolve_max_tokens_prefers_endpoint_reported() {
        // endpoint 申告がある場合はそれを採用（既知テーブルより優先）
        let resolved = resolve_max_tokens("openai/gpt-oss-20b", Some(65_536));
        assert_eq!(resolved, Some(65_536));
    }

    #[test]
    fn test_resolve_max_tokens_falls_back_to_known_table() {
        // endpoint 申告が無い場合は known テーブルから取得
        let resolved = resolve_max_tokens("openai/gpt-oss-20b", None);
        assert_eq!(resolved, Some(131_072));
    }

    #[test]
    fn test_resolve_max_tokens_returns_none_for_unknown() {
        // どちらにも無ければ None（レスポンスでは null）
        let resolved = resolve_max_tokens("totally-unknown-model", None);
        assert_eq!(resolved, None);
    }

    #[test]
    fn test_canonical_for_returns_self_for_unknown() {
        // self-canonical fallback: mapping 未登録のモデルは id 自身を canonical とする
        // （`/v1/models` レスポンスで canonical_name: null を出さない方針）
        let models = vec![("gpt-oss:20b", Some("openai/gpt-oss-20b"))];
        let res = build_canonical_maps(models.into_iter());

        assert_eq!(res.canonical_for("unknown-model"), "unknown-model");
        assert!(!res.is_known("unknown-model"));
        assert!(res.is_known("openai/gpt-oss-20b"));
        assert!(res.is_known("gpt-oss:20b"));
    }

    #[test]
    fn test_aliases_for_returns_sorted_aliases() {
        let models = vec![
            ("qwen3.5:35b-a3b", Some("Qwen/Qwen3.5-35B-A3B")),
            ("qwen/qwen3.5-35b-a3b", Some("Qwen/Qwen3.5-35B-A3B")),
        ];
        let res = build_canonical_maps(models.into_iter());
        let aliases = res.aliases_for("Qwen/Qwen3.5-35B-A3B");
        assert_eq!(aliases, vec!["qwen/qwen3.5-35b-a3b", "qwen3.5:35b-a3b"]);
    }

    #[test]
    fn test_aliases_for_returns_empty_for_unknown() {
        let res = build_canonical_maps(std::iter::empty());
        assert!(res.aliases_for("anything").is_empty());
    }
}

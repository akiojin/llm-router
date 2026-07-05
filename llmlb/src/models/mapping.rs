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

mod builtin;
pub use builtin::*;
mod repo_id;
pub use repo_id::*;
mod context_length;
pub use context_length::*;
mod quantization;
pub use quantization::*;
mod identity;
pub use identity::*;

fn model_id_eq(left: &str, right: &str) -> bool {
    left == right || left.eq_ignore_ascii_case(right)
}

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
            return model_key.to_string();
        }
        if let Some(canonical) = self.model_to_canonical.get(model_key) {
            return canonical.clone();
        }
        // 量子化サフィックス（:Q4_K_M 等）を除いた base ID で再 lookup する
        // （ggml-org の GGUF リポジトリのように、量子化バリアントが alias に未登録なケース）。
        let (base, quant) = split_quantization_suffix(model_key);
        if quant.is_some() && base != model_key {
            if self.canonical_to_aliases.contains_key(base) {
                return base.to_string();
            }
            if let Some(canonical) = self.model_to_canonical.get(base) {
                return canonical.clone();
            }
        }
        // self-canonical fallback
        model_key.to_string()
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
///
/// 2 パス構成:
/// 1. 明示 canonical（BUILTIN / sync 由来）をそのまま登録する。
/// 2. 明示 canonical を持たないモデルを identity（family/size/tuning）でグループ化し、
///    同一 identity 内に一次配布元 owner のメンバーがいればそれを canonical に採用する
///    （US-029 FR-027/FR-028）。一次配布元メンバーが不在ならマージしない（self-canonical）。
pub fn build_canonical_maps<'a>(
    models: impl Iterator<Item = (&'a str, Option<&'a str>)>,
) -> CanonicalResolution {
    let pairs: Vec<(String, Option<String>)> = models
        .map(|(id, c)| (id.to_string(), c.map(|s| s.to_string())))
        .collect();

    let mut res = CanonicalResolution::default();

    // Pass 1: 明示 canonical（既存挙動）
    for (model_id, canonical) in &pairs {
        if let Some(canonical) = canonical {
            if canonical != model_id {
                res.canonical_to_aliases
                    .entry(canonical.clone())
                    .or_default()
                    .insert(model_id.clone());
            }
            res.model_to_canonical
                .insert(model_id.clone(), canonical.clone());
        }
    }

    // Pass 2a: 各 identity グループの一次配布元 canonical アンカーを収集する。
    //
    // アンカー候補は「明示 canonical を持つモデルはその canonical（BUILTIN 由来の
    // 一次配布元 ID を含む）」「明示 canonical を持たないモデルは model_id 自身」とし、
    // いずれも owner が family の一次配布元 org のものだけを採用する。これにより、
    // BUILTIN で既知（Pass 1 で explicit canonical 化）の一次配布元モデルも、
    // 同一 identity の再配布 variant の集約先になれる（Codex review: self-canonical
    // first-party の取りこぼし防止）。
    let mut anchors: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    for (model_id, canonical) in &pairs {
        let candidate: &str = canonical.as_deref().unwrap_or(model_id.as_str());
        let ident = parse_identity(candidate);
        // size 不明（embeddings 等）は誤マージ回避のためアンカーにしない
        if ident.size.is_none() {
            continue;
        }
        let Some(fp) = first_party_org_for_family(&ident.family) else {
            continue;
        };
        if !owner_of(candidate)
            .map(|o| o.eq_ignore_ascii_case(fp))
            .unwrap_or(false)
        {
            continue;
        }
        // 決定的に最小の candidate を採用
        anchors
            .entry(ident.group_key())
            .and_modify(|existing| {
                if candidate < existing.as_str() {
                    *existing = candidate.to_string();
                }
            })
            .or_insert_with(|| candidate.to_string());
    }

    // Pass 2b: 明示 canonical を持たないモデルを、同一 identity の一次配布元アンカーへ集約する。
    // 一次配布元アンカーが不在のグループはマージしない（self-canonical のまま）。
    for (model_id, canonical) in &pairs {
        if canonical.is_some() {
            continue;
        }
        let ident = parse_identity(model_id);
        if ident.size.is_none() {
            continue;
        }
        let Some(anchor) = anchors.get(&ident.group_key()) else {
            continue;
        };
        res.model_to_canonical
            .insert(model_id.clone(), anchor.clone());
        if model_id != anchor {
            res.canonical_to_aliases
                .entry(anchor.clone())
                .or_default()
                .insert(model_id.clone());
        }
    }

    res
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
mod tests;

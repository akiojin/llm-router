//! US-029 論理モデル identity のヒューリスティック解析（family/size/tuning）と一次配布元判定
//!
//! arch-review [H6]: models/mapping.rs から identity 推定ロジックを分離。
//! parse_identity は量子化サフィックス除去に親の split_quantization_suffix を用いる。

use super::split_quantization_suffix;

/// 論理モデルの identity（family, size/arch, tuning）。
///
/// US-029: 同じ論理モデルを指す表記を「サイズ・チューニング」で区別し、
/// owner・量子化・大文字小文字は含めない（variant 扱い）。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ModelIdentity {
    /// 例: `gemma-4`, `qwen3-coder`, `llama-3.3`
    pub family: String,
    /// 例: `e4b`, `26b-a4b`, `27b`（未検出なら `None`）
    pub size: Option<String>,
    /// instruction-tuned（`-it` / `-instruct`）なら true
    pub instruct: bool,
}

impl ModelIdentity {
    /// グルーピング用の正規化キー（family|size|tuning）。
    pub fn group_key(&self) -> String {
        format!(
            "{}|{}|{}",
            self.family,
            self.size.as_deref().unwrap_or("-"),
            if self.instruct { "it" } else { "base" }
        )
    }
}

/// 配布形式・量子化方式を表す末尾サフィックス（identity から除去する）。
const FORMAT_SUFFIXES: &[&str] = &[
    "-gguf", "-awq", "-gptq", "-mlx", "-bnb", "-fp8", "-int4", "-int8", "-hf",
];

/// instruction-tuned を示す末尾マーカー（長いものから判定する）。
const INSTRUCT_MARKERS: &[&str] = &["-instruction-tuned", "-instruct", "-it"];

/// family 接頭辞 → 一次配布元（公式 HF org）。
///
/// US-029 FR-027: 同一 identity に複数オーナーがある場合の canonical 選定基準。
const FIRST_PARTY_ORGS: &[(&str, &str)] = &[
    ("gemma", "google"),
    ("gpt-oss", "openai"),
    ("qwen", "Qwen"),
    ("llama", "meta-llama"),
    ("nemotron", "nvidia"),
    ("glm", "zai-org"),
    ("nomic", "nomic-ai"),
    ("phi", "microsoft"),
    ("mistral", "mistralai"),
    ("mixtral", "mistralai"),
    ("deepseek", "deepseek-ai"),
];

/// family から一次配布元 org を返す（接頭辞一致）。判別不能なら `None`。
pub fn first_party_org_for_family(family: &str) -> Option<&'static str> {
    let f = family.to_ascii_lowercase();
    FIRST_PARTY_ORGS
        .iter()
        .find(|(prefix, _)| f.starts_with(prefix))
        .map(|(_, org)| *org)
}

/// `27b` / `e4b` / `120b` などのサイズトークンか判定する（`a4b` 等の active 部も該当）。
fn is_size_token(t: &str) -> bool {
    // `e2b` `e4b` の effective 接頭辞、`a4b` `a3b` の MoE active 接頭辞を許容
    let t = t
        .strip_prefix('e')
        .or_else(|| t.strip_prefix('a'))
        .unwrap_or(t);
    let Some(core) = t.strip_suffix('b') else {
        return false;
    };
    if core.is_empty() {
        return false;
    }
    let mut seen_dot = false;
    for c in core.chars() {
        if c == '.' {
            if seen_dot {
                return false;
            }
            seen_dot = true;
        } else if !c.is_ascii_digit() {
            return false;
        }
    }
    true
}

/// repo 名等から論理モデル identity をヒューリスティック抽出する（best-effort）。
///
/// owner と量子化サフィックスを除去し、format サフィックス（`-GGUF` 等）と
/// instruct マーカーを分離して family / size / instruct を推定する。
/// US-029 FR-028: 未知モデルのグルーピング・canonical 推定に用いる。
pub fn parse_identity(model_id: &str) -> ModelIdentity {
    let (base, _q) = split_quantization_suffix(model_id);
    let repo = base.rsplit('/').next().unwrap_or(base);
    let mut s = repo.to_ascii_lowercase();

    // 末尾の format サフィックスを除去（複数付与に備えてループ）
    loop {
        let mut changed = false;
        for suf in FORMAT_SUFFIXES {
            if let Some(stripped) = s.strip_suffix(suf) {
                s = stripped.to_string();
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }

    // instruct マーカー（長い順）
    let mut instruct = false;
    for marker in INSTRUCT_MARKERS {
        if let Some(stripped) = s.strip_suffix(marker) {
            instruct = true;
            s = stripped.to_string();
            break;
        }
    }

    let tokens: Vec<&str> = s.split('-').filter(|t| !t.is_empty()).collect();
    // 先頭に現れる最初のサイズトークン以降を size、その手前を family とする
    let size_idx = tokens.iter().position(|t| is_size_token(t));
    let (family, size) = match size_idx {
        Some(i) => (tokens[..i].join("-"), Some(tokens[i..].join("-"))),
        None => (tokens.join("-"), None),
    };

    ModelIdentity {
        family,
        size,
        instruct,
    }
}

/// model_id の owner（`/` の前）を返す。owner が無ければ `None`。
pub(super) fn owner_of(model_id: &str) -> Option<&str> {
    let (base, _q) = split_quantization_suffix(model_id);
    base.rsplit_once('/').map(|(owner, _)| owner)
}

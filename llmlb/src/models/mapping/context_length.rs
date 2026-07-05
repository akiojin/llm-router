//! 既知モデルの context window（max_tokens）フォールバックテーブルと解決関数
//!
//! arch-review [H6]: models/mapping.rs から自己完結した単一関心を分離。
//! 呼び出し側は親の `pub use context_length::*` 再エクスポート経由で参照する。

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

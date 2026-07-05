//! 量子化サフィックス（`:Q4_K_M` `:F16` 等）の検出・分割ユーティリティ
//!
//! arch-review [H6]: models/mapping.rs から凝集した leaf util を分離。
//! 呼び出し側は親の `pub use quantization::*` 再エクスポート経由で参照する。

/// 量子化サフィックス（`:Q4_K_M` `:Q8_0` `:F16` 等）をモデル ID から分離する。
///
/// 戻り値は `(base_id, quantization)` のタプル。suffix が量子化タグに該当する場合は
/// `:` の前後に分割し、そうでなければ元の `model_id` 全体を base として返す
/// （`qwen3-coder:30b` のような Ollama 形式タグや `gemma3:latest` を誤検出しない）。
///
/// G-3 の暫定対応として、ID は当面互換性のため変更せず、量子化情報は新フィールド
/// `quantization` として API レスポンスに別出しする。
pub fn split_quantization_suffix(model_id: &str) -> (&str, Option<&str>) {
    if let Some(idx) = model_id.rfind(':') {
        let suffix = &model_id[idx + 1..];
        if is_quantization_tag(suffix) {
            return (&model_id[..idx], Some(suffix));
        }
    }
    (model_id, None)
}

/// 与えられた文字列が GGUF / safetensors の量子化タグらしいかを判定する。
///
/// 認識する形式:
/// - `Q[0-9]...`: `Q4_K_M`, `Q5_K_M`, `Q8_0` など（GGUF k-quants/legacy quants）
/// - `IQ[0-9]...`: `IQ4_XS`, `IQ3_S` など（GGUF imatrix quants）
/// - 浮動小数点フォーマット: `F16`, `F32`, `BF16`, `FP16`, `FP32`, `F8E4M3FN`, `F8E5M2`
fn is_quantization_tag(s: &str) -> bool {
    if matches!(
        s,
        "F16" | "F32" | "BF16" | "FP16" | "FP32" | "F8E4M3FN" | "F8E5M2"
    ) {
        return true;
    }
    let mut chars = s.chars();
    match chars.next() {
        Some('Q') => chars.next().is_some_and(|c| c.is_ascii_digit()),
        Some('I') => chars.next() == Some('Q'),
        _ => false,
    }
}

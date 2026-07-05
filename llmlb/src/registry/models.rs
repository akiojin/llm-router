//! モデル情報管理
//!
//! LLM runtimeモデルのメタデータ管理

// ModelInfo / ModelSource は中核ドメイン型として types/model.rs へ移設した
// （db → registry の逆依存/循環を避けるため）。既存の
// `crate::registry::models::{ModelInfo, ModelSource}` 参照との後方互換のため re-export する。
pub use crate::types::model::{ModelInfo, ModelSource};

// arch-review [L13]: `extract_repo_id` は純粋 util のため共有層 models/mapping.rs へ
// 移設した。既存の `registry::models::extract_repo_id` 参照との後方互換のため re-export する。
pub use crate::models::mapping::extract_repo_id;

/// HuggingFaceリポジトリ名からモデルIDを生成（階層形式）
///
/// SPEC-dcaeaec4 FR-2に準拠:
/// - `openai/gpt-oss-20b` → `openai/gpt-oss-20b`
/// - `TheBloke/Llama-2-7B-GGUF` → `thebloke/llama-2-7b-gguf`
///
/// 正規化ルール:
/// 1. 小文字に変換
/// 2. 先頭・末尾のスラッシュを除去
/// 3. 危険なパターン (`..`, `\0`) は "_latest" に変換
pub fn generate_model_id(repo: &str) -> String {
    if repo.is_empty() {
        return "_latest".into();
    }

    // 危険なパターンをチェック
    if repo.contains("..") || repo.contains('\0') {
        return "_latest".into();
    }

    // 小文字に変換し、先頭・末尾のスラッシュを除去
    let normalized = repo.to_lowercase();
    let trimmed = normalized.trim_matches('/');

    if trimmed.is_empty() {
        "_latest".into()
    } else {
        trimmed.to_string()
    }
}

#[cfg(test)]
mod tests;

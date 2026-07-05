//! HuggingFace URL から repo_id を抽出する純粋ユーティリティ
//!
//! arch-review [H6/L13]: models/mapping.rs から独立した純粋 util を分離。
//! 呼び出し側は親の `pub use repo_id::*` 再エクスポート経由で参照する。

/// HuggingFace URLからrepo_idを抽出
///
/// 入力例:
/// - "https://huggingface.co/openai/gpt-oss-20b" → "openai/gpt-oss-20b"
/// - "http://huggingface.co/openai/gpt-oss-20b" → "openai/gpt-oss-20b"
/// - "openai/gpt-oss-20b" → "openai/gpt-oss-20b" (そのまま)
/// - "gpt-oss-20b" → "gpt-oss-20b" (そのまま)
///
/// 備考:
/// - huggingface_hubのsnapshot_downloadはrepo_id形式（namespace/repo_name）を期待する
/// - フルURLが渡された場合はrepo_id部分のみを抽出して返す
///
/// arch-review [L13]: registry 層にあった純粋 util を共有 util (models/mapping) へ移設し、
/// アウトバウンドアダプタ(metadata)が registry へ逆依存する状態を解消した。
pub fn extract_repo_id(input: &str) -> String {
    // HuggingFace URLパターンを検出
    let hf_patterns = [
        "https://huggingface.co/",
        "http://huggingface.co/",
        "https://www.huggingface.co/",
        "http://www.huggingface.co/",
    ];

    for pattern in hf_patterns {
        if let Some(rest) = input.strip_prefix(pattern) {
            // URLの残り部分からrepo_idを抽出
            // "openai/gpt-oss-20b/tree/main" → "openai/gpt-oss-20b"
            let parts: Vec<&str> = rest.split('/').collect();
            if parts.len() >= 2 {
                // namespace/repo_name を返す
                return format!("{}/{}", parts[0], parts[1]);
            } else if parts.len() == 1 && !parts[0].is_empty() {
                return parts[0].to_string();
            }
        }
    }

    // HF_BASE_URL環境変数が設定されている場合、そのURLも考慮
    if let Ok(base_url) = std::env::var("HF_BASE_URL") {
        let base_url = base_url.trim_end_matches('/');
        let patterns = [
            format!("{}/", base_url),
            format!("{}//", base_url.replace("https://", "http://")),
        ];
        for pattern in patterns {
            if let Some(rest) = input.strip_prefix(&pattern) {
                let parts: Vec<&str> = rest.split('/').collect();
                if parts.len() >= 2 {
                    return format!("{}/{}", parts[0], parts[1]);
                } else if parts.len() == 1 && !parts[0].is_empty() {
                    return parts[0].to_string();
                }
            }
        }
    }

    // URLパターンに一致しない場合はそのまま返す
    input.to_string()
}

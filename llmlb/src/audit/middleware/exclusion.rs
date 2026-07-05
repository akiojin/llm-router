//! 監査ログ対象外とする HTTP パスの判定
//!
//! arch-review [H6] round2: audit/middleware.rs からパス除外判定を分離。

/// 監査対象から除外すべきパスか判定する
pub(super) fn should_exclude(path: &str) -> bool {
    // WebSocket
    if path.starts_with("/ws/") || path == "/ws" {
        return true;
    }
    // ヘルスチェック
    if path == "/health" {
        return true;
    }
    // 静的アセット（ダッシュボード配下の拡張子付きファイル）
    if path.starts_with("/dashboard/") {
        let extensions = [
            ".js", ".css", ".png", ".jpg", ".svg", ".ico", ".woff", ".woff2", ".map",
        ];
        if extensions.iter().any(|ext| path.ends_with(ext)) {
            return true;
        }
    }
    // ダッシュボードSSEポーリング
    if path == "/api/dashboard/events" {
        return true;
    }
    false
}

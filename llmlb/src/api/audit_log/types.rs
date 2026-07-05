//! 監査ログAPIのワイヤー型とクエリ変換 (SPEC-8301d106)

use super::{MAX_AUDIT_PAGE, MAX_AUDIT_PER_PAGE};
use crate::audit::types::{AuditLogEntry, AuditLogFilter};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// 監査ログ一覧取得のクエリパラメータ
#[derive(Debug, Deserialize)]
pub struct AuditLogQueryParams {
    /// アクター種別でフィルタ
    pub actor_type: Option<String>,
    /// アクターIDでフィルタ
    pub actor_id: Option<String>,
    /// HTTPメソッドでフィルタ
    pub http_method: Option<String>,
    /// リクエストパスでフィルタ
    pub request_path: Option<String>,
    /// ステータスコードでフィルタ
    pub status_code: Option<u16>,
    /// 開始日時
    pub time_from: Option<DateTime<Utc>>,
    /// 終了日時
    pub time_to: Option<DateTime<Utc>>,
    /// クライアントIPアドレス（LIKE前方一致）
    pub ip: Option<String>,
    /// フリーテキスト検索
    pub search: Option<String>,
    /// ページ番号（1始まり、デフォルト: 1）
    pub page: Option<i64>,
    /// ページあたり件数（デフォルト: 50）
    pub per_page: Option<i64>,
    /// アーカイブDBも検索対象に含める
    pub include_archive: Option<bool>,
    /// 出力フォーマット（デフォルト: json、csv等は将来対応）
    pub format: Option<String>,
}

impl From<AuditLogQueryParams> for AuditLogFilter {
    fn from(params: AuditLogQueryParams) -> Self {
        Self {
            actor_type: params.actor_type,
            actor_id: params.actor_id,
            http_method: params.http_method,
            request_path: params.request_path,
            status_code: params.status_code,
            time_from: params.time_from,
            time_to: params.time_to,
            client_ip: params.ip,
            search_text: params.search,
            // page は上限をクランプして offset 乗算のオーバーフローを防ぐ
            page: params.page.map(|p| p.clamp(1, MAX_AUDIT_PAGE)),
            // per_page は上限をクランプして全行のメモリ展開を防ぐ
            per_page: params.per_page.map(|p| p.clamp(1, MAX_AUDIT_PER_PAGE)),
            include_archive: params.include_archive,
        }
    }
}

/// 監査ログ一覧レスポンス
#[derive(Debug, Serialize, Deserialize)]
pub struct AuditLogListResponse {
    /// 監査ログエントリ一覧
    pub items: Vec<AuditLogEntry>,
    /// 総件数
    pub total: i64,
    /// 現在のページ番号
    pub page: i64,
    /// ページあたり件数
    pub per_page: i64,
}

/// 監査ログ統計レスポンス
#[derive(Debug, Serialize, Deserialize)]
pub struct AuditLogStatsResponse {
    /// 総エントリ数
    pub total_entries: i64,
    /// HTTPメソッド別カウント
    pub by_method: Vec<MethodCount>,
    /// アクター種別カウント
    pub by_actor_type: Vec<ActorTypeCount>,
    /// 直近24時間のエントリ数
    pub last_24h: i64,
}

/// HTTPメソッド別カウント
#[derive(Debug, Serialize, Deserialize)]
pub struct MethodCount {
    /// HTTPメソッド名
    pub method: String,
    /// カウント
    pub count: i64,
}

/// アクター種別カウント
#[derive(Debug, Serialize, Deserialize)]
pub struct ActorTypeCount {
    /// アクター種別
    pub actor_type: String,
    /// カウント
    pub count: i64,
}

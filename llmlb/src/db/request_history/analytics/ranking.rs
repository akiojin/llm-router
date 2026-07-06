//! クライアントIPランキングとリクエスト数集計。
//!
//! IP 別のリクエスト数降順ランキング（IPv6 は /64 プレフィックス集計）と、
//! 閾値チェック用の期間内 IP 別リクエスト数を提供する。

use crate::common::error::{LbError, RouterResult};
use crate::db::request_history::RequestHistoryStorage;
use chrono::{Duration, Utc};

/// IPランキングの1エントリ
#[derive(Debug, Clone, serde::Serialize)]
pub struct ClientIpRanking {
    /// IPアドレス（IPv6は/64プレフィックス）
    pub ip: String,
    /// リクエスト数
    pub request_count: i64,
    /// 最終アクセス時刻
    pub last_seen: String,
    /// 閾値超過フラグ（デフォルトfalse、API層で設定）
    pub is_alert: bool,
    /// 使用APIキー数
    pub api_key_count: i64,
}

/// IPランキングの集計結果
#[derive(Debug, Clone, serde::Serialize)]
pub struct ClientIpRankingResult {
    /// ランキング一覧
    pub rankings: Vec<ClientIpRanking>,
    /// 総件数
    pub total_count: usize,
    /// 現在のページ番号
    pub page: usize,
    /// 1ページあたりの件数
    pub per_page: usize,
}

/// SQLiteから取得したIP集計行
#[derive(sqlx::FromRow)]
struct ClientIpRow {
    client_ip: Option<String>,
    request_count: i64,
    last_seen: String,
}

impl RequestHistoryStorage {
    /// IPランキングを取得（リクエスト数降順、ページネーション付き）
    ///
    /// IPv6アドレスは/64プレフィックスでグルーピングして集計する。
    pub async fn get_client_ip_ranking(
        &self,
        hours: u32,
        page: usize,
        per_page: usize,
        ip_prefix: Option<&str>,
    ) -> RouterResult<ClientIpRankingResult> {
        let cutoff = Utc::now() - Duration::hours(hours as i64);
        let cutoff_str = cutoff.to_rfc3339();

        // SQLiteでIPごとの集計を取得（NULLのclient_ipは除外）
        let query_str = if ip_prefix.is_some() {
            "SELECT client_ip, COUNT(*) as request_count,
                    MAX(timestamp) as last_seen
             FROM request_history
             WHERE client_ip IS NOT NULL AND timestamp >= ? AND client_ip LIKE ? || '%'
             GROUP BY client_ip
             ORDER BY request_count DESC"
        } else {
            "SELECT client_ip, COUNT(*) as request_count,
                    MAX(timestamp) as last_seen
             FROM request_history
             WHERE client_ip IS NOT NULL AND timestamp >= ?
             GROUP BY client_ip
             ORDER BY request_count DESC"
        };

        let mut query = sqlx::query_as::<_, ClientIpRow>(query_str);
        query = query.bind(&cutoff_str);
        if let Some(prefix) = ip_prefix {
            query = query.bind(prefix);
        }

        let rows: Vec<ClientIpRow> = query
            .fetch_all(&self.pool)
            .await
            .map_err(|e| LbError::Database(e.to_string()))?;

        // IPv6 /64グルーピング: Rust側で集約
        use std::collections::HashMap;
        let mut grouped: HashMap<String, (i64, String)> = HashMap::new();

        for row in &rows {
            let ip_str = match &row.client_ip {
                Some(ip) => ip.as_str(),
                None => continue,
            };
            let key = crate::common::ip::ipv6_to_prefix64(ip_str);

            let entry = grouped.entry(key).or_insert_with(|| (0, String::new()));
            entry.0 += row.request_count;
            // last_seenは最新を保持
            if entry.1.is_empty() || row.last_seen > entry.1 {
                entry.1.clone_from(&row.last_seen);
            }
        }

        let mut rankings: Vec<ClientIpRanking> = grouped
            .into_iter()
            .map(|(ip, (count, last_seen))| ClientIpRanking {
                ip,
                request_count: count,
                last_seen,
                is_alert: false,
                api_key_count: 0, // Phase 8で正確な集計に更新
            })
            .collect();

        // リクエスト数降順ソート
        rankings.sort_by_key(|r| std::cmp::Reverse(r.request_count));

        let total_count = rankings.len();
        let offset = page.saturating_sub(1).saturating_mul(per_page);
        let paginated = rankings.into_iter().skip(offset).take(per_page).collect();

        Ok(ClientIpRankingResult {
            rankings: paginated,
            total_count,
            page,
            per_page,
        })
    }

    /// 過去N時間のIP別リクエスト数を取得（閾値チェック用）
    ///
    /// IPv6は/64プレフィックスでグルーピングして集計する。
    pub async fn get_ip_request_counts_since(
        &self,
        hours: u32,
    ) -> RouterResult<std::collections::HashMap<String, i64>> {
        let cutoff = Utc::now() - Duration::hours(hours as i64);
        let cutoff_str = cutoff.to_rfc3339();

        let rows: Vec<ClientIpRow> = sqlx::query_as(
            "SELECT client_ip, COUNT(*) as request_count,
                    MAX(timestamp) as last_seen
             FROM request_history
             WHERE client_ip IS NOT NULL AND timestamp >= ?
             GROUP BY client_ip",
        )
        .bind(&cutoff_str)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| LbError::Database(e.to_string()))?;

        let mut counts: std::collections::HashMap<String, i64> = std::collections::HashMap::new();
        for row in &rows {
            let ip_str = match &row.client_ip {
                Some(ip) => ip.as_str(),
                None => continue,
            };
            let key = crate::common::ip::ipv6_to_prefix64(ip_str);
            *counts.entry(key).or_insert(0) += row.request_count;
        }

        Ok(counts)
    }
}

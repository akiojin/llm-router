//! リクエスト履歴のクライアントIP分析
//!
//! arch-review [H3]: RequestHistoryStorage の肥大化を抑えるため、永続化コアから
//! 分離できるクライアントIP分析メソッド群（ranking / timeline / heatmap / detail /
//! distribution / api-keys）と専用型を submodule へ切り出した。メソッドは
//! `RequestHistoryStorage` の impl として維持し、公開型は親モジュールで re-export する。

use super::RequestHistoryStorage;
use crate::common::error::{LbError, RouterResult};
use chrono::{Duration, Utc};
use std::net::IpAddr;

mod ranking;
pub use ranking::{ClientIpRanking, ClientIpRankingResult};

#[derive(Debug, Clone)]
enum ClientIpFilter {
    Exact(String),
    Ipv6Prefix64(String),
}

impl ClientIpFilter {
    fn from_input(ip: &str) -> Self {
        if let Some(raw_prefix) = ip.strip_suffix("/64") {
            if let Ok(v6) = raw_prefix.parse::<std::net::Ipv6Addr>() {
                return Self::Ipv6Prefix64(format!("{v6}/64"));
            }
        }

        if let Ok(parsed_ip) = ip.parse::<IpAddr>() {
            return Self::Exact(parsed_ip.to_string());
        }

        Self::Exact(ip.to_string())
    }
}

#[derive(sqlx::FromRow)]
struct ClientIpValueRow {
    client_ip: String,
}

fn build_in_clause(column: &str, count: usize) -> String {
    let placeholders = std::iter::repeat_n("?", count)
        .collect::<Vec<_>>()
        .join(", ");
    format!("{column} IN ({placeholders})")
}

impl RequestHistoryStorage {
    async fn resolve_client_ips_for_filter(&self, ip: &str) -> RouterResult<Vec<String>> {
        let filter = ClientIpFilter::from_input(ip);

        match filter {
            ClientIpFilter::Exact(exact_ip) => Ok(vec![exact_ip]),
            ClientIpFilter::Ipv6Prefix64(prefix64) => {
                let rows: Vec<ClientIpValueRow> = sqlx::query_as(
                    "SELECT DISTINCT client_ip
                     FROM request_history
                     WHERE client_ip IS NOT NULL",
                )
                .fetch_all(&self.pool)
                .await
                .map_err(|e| LbError::Database(e.to_string()))?;

                Ok(rows
                    .into_iter()
                    .map(|row| row.client_ip)
                    .filter(|stored_ip| crate::common::ip::ipv6_to_prefix64(stored_ip) == prefix64)
                    .collect())
            }
        }
    }

    /// ユニークIP数の1時間刻みタイムラインを取得
    ///
    /// 指定時間数分の24ポイント（各時間帯のユニークIP数）を返す。
    /// データがない時間帯はunique_ips=0で埋める。
    pub async fn get_unique_ip_timeline(
        &self,
        hours: u32,
    ) -> RouterResult<Vec<UniqueIpTimelinePoint>> {
        let now = Utc::now();
        let cutoff = now - Duration::hours(hours as i64);
        let cutoff_str = cutoff.to_rfc3339();

        let rows: Vec<TimelineClientRow> = sqlx::query_as(
            "SELECT strftime('%Y-%m-%dT%H:00:00', timestamp) as hour,
                    client_ip
             FROM request_history
             WHERE client_ip IS NOT NULL AND timestamp >= ?
             GROUP BY strftime('%Y-%m-%dT%H:00:00', timestamp), client_ip
             ORDER BY hour ASC",
        )
        .bind(&cutoff_str)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| LbError::Database(e.to_string()))?;

        // 24時間分のポイントを生成（データがない時間帯は0埋め）
        use std::collections::{HashMap, HashSet};
        let mut grouped: HashMap<String, HashSet<String>> = HashMap::new();
        for row in rows {
            let normalized_ip = crate::common::ip::ipv6_to_prefix64(&row.client_ip);
            grouped.entry(row.hour).or_default().insert(normalized_ip);
        }
        let row_map: HashMap<String, i64> = grouped
            .into_iter()
            .map(|(hour, unique_ips)| (hour, unique_ips.len() as i64))
            .collect();

        let mut timeline = Vec::with_capacity(hours as usize);
        for h in (1..=hours).rev() {
            let point_time = now - Duration::hours(h as i64);
            let hour_key = point_time.format("%Y-%m-%dT%H:00:00").to_string();
            let unique_ips = row_map.get(&hour_key).copied().unwrap_or(0);
            timeline.push(UniqueIpTimelinePoint {
                hour: hour_key,
                unique_ips,
            });
        }

        Ok(timeline)
    }

    /// モデル別リクエスト分布を取得
    ///
    /// 指定時間数内のモデル別リクエスト数とパーセンテージを返す。
    pub async fn get_model_distribution_by_clients(
        &self,
        hours: u32,
    ) -> RouterResult<Vec<ModelDistribution>> {
        let cutoff = Utc::now() - Duration::hours(hours as i64);
        let cutoff_str = cutoff.to_rfc3339();

        let rows: Vec<ModelDistRow> = sqlx::query_as(
            "SELECT model, COUNT(*) as request_count
             FROM request_history
             WHERE client_ip IS NOT NULL AND timestamp >= ?
             GROUP BY model
             ORDER BY request_count DESC",
        )
        .bind(&cutoff_str)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| LbError::Database(e.to_string()))?;

        let total: i64 = rows.iter().map(|r| r.request_count).sum();

        let distributions = rows
            .into_iter()
            .map(|r| {
                let percentage = if total > 0 {
                    (r.request_count as f64 / total as f64) * 100.0
                } else {
                    0.0
                };
                ModelDistribution {
                    model: r.model,
                    request_count: r.request_count,
                    percentage: (percentage * 10.0).round() / 10.0, // 小数点1位
                }
            })
            .collect();

        Ok(distributions)
    }

    /// リクエストヒートマップを取得（曜日×時間帯）
    ///
    /// 指定時間数内のリクエストを曜日(0-6)×時間帯(0-23)で集計。
    /// データがないセルはcount=0で埋める。
    pub async fn get_request_heatmap(
        &self,
        hours: u32,
        ip_prefix: Option<&str>,
    ) -> RouterResult<Vec<HeatmapCell>> {
        let cutoff = Utc::now() - Duration::hours(hours as i64);
        let cutoff_str = cutoff.to_rfc3339();

        let query_str = if ip_prefix.is_some() {
            "SELECT CAST(strftime('%w', timestamp) AS INTEGER) as day_of_week,
                    CAST(strftime('%H', timestamp) AS INTEGER) as hour,
                    COUNT(*) as count
             FROM request_history
             WHERE client_ip IS NOT NULL AND timestamp >= ? AND client_ip LIKE ? || '%'
             GROUP BY strftime('%w', timestamp), strftime('%H', timestamp)"
        } else {
            "SELECT CAST(strftime('%w', timestamp) AS INTEGER) as day_of_week,
                    CAST(strftime('%H', timestamp) AS INTEGER) as hour,
                    COUNT(*) as count
             FROM request_history
             WHERE client_ip IS NOT NULL AND timestamp >= ?
             GROUP BY strftime('%w', timestamp), strftime('%H', timestamp)"
        };

        let mut query = sqlx::query_as::<_, HeatmapRow>(query_str);
        query = query.bind(&cutoff_str);
        if let Some(prefix) = ip_prefix {
            query = query.bind(prefix);
        }

        let rows: Vec<HeatmapRow> = query
            .fetch_all(&self.pool)
            .await
            .map_err(|e| LbError::Database(e.to_string()))?;

        // 7×24 = 168セルの完全マトリックスを生成
        use std::collections::HashMap;
        let row_map: HashMap<(i64, i64), i64> = rows
            .into_iter()
            .map(|r| ((r.day_of_week, r.hour), r.count))
            .collect();

        let mut cells = Vec::with_capacity(168);
        for dow in 0..7 {
            for h in 0..24 {
                let count = row_map.get(&(dow, h)).copied().unwrap_or(0);
                cells.push(HeatmapCell {
                    day_of_week: dow,
                    hour: h,
                    count,
                });
            }
        }

        Ok(cells)
    }

    /// 特定IPのドリルダウン詳細を取得
    pub async fn get_client_detail(&self, ip: &str, limit: usize) -> RouterResult<ClientDetail> {
        let matched_ips = self.resolve_client_ips_for_filter(ip).await?;
        if matched_ips.is_empty() {
            return Ok(ClientDetail::empty());
        }

        let where_clause = build_in_clause("client_ip", matched_ips.len());

        // 合計リクエスト数・初回/最終アクセス
        let summary_sql = format!(
            "SELECT COUNT(*) as total_requests,
                    MIN(timestamp) as first_seen,
                    MAX(timestamp) as last_seen
             FROM request_history
             WHERE {where_clause}"
        );
        let mut summary_query = sqlx::query_as::<_, ClientSummaryRow>(&summary_sql);
        for matched_ip in &matched_ips {
            summary_query = summary_query.bind(matched_ip);
        }
        let summary: Option<ClientSummaryRow> = summary_query
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| LbError::Database(e.to_string()))?;

        let (total_requests, first_seen, last_seen) = match summary {
            Some(s) if s.total_requests > 0 => (s.total_requests, s.first_seen, s.last_seen),
            _ => return Ok(ClientDetail::empty()),
        };

        // 直近リクエスト
        let recent_sql = format!(
            "SELECT id, timestamp, model, status, duration_ms
             FROM request_history
             WHERE {where_clause}
             ORDER BY timestamp DESC
             LIMIT ?"
        );
        let mut recent_query = sqlx::query_as::<_, ClientRecentRequestRow>(&recent_sql);
        for matched_ip in &matched_ips {
            recent_query = recent_query.bind(matched_ip);
        }
        let recent: Vec<ClientRecentRequestRow> = recent_query
            .bind(limit as i64)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| LbError::Database(e.to_string()))?;

        let recent_requests: Vec<ClientRecentRequest> = recent
            .into_iter()
            .map(|r| ClientRecentRequest {
                id: r.id,
                timestamp: r.timestamp,
                model: r.model,
                status: r.status,
                duration_ms: r.duration_ms,
            })
            .collect();

        // モデル分布
        let model_sql = format!(
            "SELECT model, COUNT(*) as request_count
             FROM request_history
             WHERE {where_clause}
             GROUP BY model
             ORDER BY request_count DESC"
        );
        let mut model_query = sqlx::query_as::<_, ModelDistRow>(&model_sql);
        for matched_ip in &matched_ips {
            model_query = model_query.bind(matched_ip);
        }
        let model_rows: Vec<ModelDistRow> = model_query
            .fetch_all(&self.pool)
            .await
            .map_err(|e| LbError::Database(e.to_string()))?;

        let total_for_pct: i64 = model_rows.iter().map(|r| r.request_count).sum();
        let model_distribution: Vec<ModelDistribution> = model_rows
            .into_iter()
            .map(|r| {
                let pct = if total_for_pct > 0 {
                    (r.request_count as f64 / total_for_pct as f64) * 100.0
                } else {
                    0.0
                };
                ModelDistribution {
                    model: r.model,
                    request_count: r.request_count,
                    percentage: (pct * 10.0).round() / 10.0,
                }
            })
            .collect();

        // 時間帯パターン（24時間）
        let hourly_sql = format!(
            "SELECT CAST(strftime('%H', timestamp) AS INTEGER) as hour,
                    COUNT(*) as count
             FROM request_history
             WHERE {where_clause}
             GROUP BY strftime('%H', timestamp)
             ORDER BY hour ASC"
        );
        let mut hourly_query = sqlx::query_as::<_, HourlyPatternRow>(&hourly_sql);
        for matched_ip in &matched_ips {
            hourly_query = hourly_query.bind(matched_ip);
        }
        let hourly_rows: Vec<HourlyPatternRow> = hourly_query
            .fetch_all(&self.pool)
            .await
            .map_err(|e| LbError::Database(e.to_string()))?;

        use std::collections::HashMap;
        let hourly_map: HashMap<i64, i64> =
            hourly_rows.into_iter().map(|r| (r.hour, r.count)).collect();
        let hourly_pattern: Vec<HourlyPattern> = (0..24)
            .map(|h| HourlyPattern {
                hour: h,
                count: hourly_map.get(&h).copied().unwrap_or(0),
            })
            .collect();

        Ok(ClientDetail {
            total_requests,
            first_seen,
            last_seen,
            recent_requests,
            model_distribution,
            hourly_pattern,
        })
    }

    /// 特定IPのAPIキー別リクエスト数を取得
    pub async fn get_client_api_keys(&self, ip: &str) -> RouterResult<Vec<ClientApiKeyUsage>> {
        let matched_ips = self.resolve_client_ips_for_filter(ip).await?;
        if matched_ips.is_empty() {
            return Ok(vec![]);
        }
        let where_clause = build_in_clause("rh.client_ip", matched_ips.len());
        let sql = format!(
            "SELECT rh.api_key_id, ak.name as key_name, COUNT(*) as request_count
             FROM request_history rh
             LEFT JOIN api_keys ak ON rh.api_key_id = ak.id
             WHERE {where_clause} AND rh.api_key_id IS NOT NULL
             GROUP BY rh.api_key_id
             ORDER BY request_count DESC"
        );
        let mut query = sqlx::query_as::<_, ClientApiKeyRow>(&sql);
        for matched_ip in &matched_ips {
            query = query.bind(matched_ip);
        }
        let rows: Vec<ClientApiKeyRow> = query
            .fetch_all(&self.pool)
            .await
            .map_err(|e| LbError::Database(e.to_string()))?;

        Ok(rows
            .into_iter()
            .map(|r| ClientApiKeyUsage {
                api_key_id: r.api_key_id,
                name: r.key_name,
                request_count: r.request_count,
            })
            .collect())
    }
}

/// APIキー別リクエスト数
#[derive(Debug, Clone, serde::Serialize)]
pub struct ClientApiKeyUsage {
    /// APIキーID
    pub api_key_id: String,
    /// キー名（削除済みの場合None）
    pub name: Option<String>,
    /// リクエスト数
    pub request_count: i64,
}

/// SQLiteから取得したAPIキー集計行
#[derive(sqlx::FromRow)]
struct ClientApiKeyRow {
    api_key_id: String,
    key_name: Option<String>,
    request_count: i64,
}

/// IPドリルダウン詳細
#[derive(Debug, Clone, serde::Serialize)]
pub struct ClientDetail {
    /// 合計リクエスト数
    pub total_requests: i64,
    /// 初回アクセス時刻
    pub first_seen: Option<String>,
    /// 最終アクセス時刻
    pub last_seen: Option<String>,
    /// 直近リクエスト
    pub recent_requests: Vec<ClientRecentRequest>,
    /// モデル分布
    pub model_distribution: Vec<ModelDistribution>,
    /// 時間帯パターン（24時間）
    pub hourly_pattern: Vec<HourlyPattern>,
}

impl ClientDetail {
    fn empty() -> Self {
        Self {
            total_requests: 0,
            first_seen: None,
            last_seen: None,
            recent_requests: vec![],
            model_distribution: vec![],
            hourly_pattern: (0..24)
                .map(|h| HourlyPattern { hour: h, count: 0 })
                .collect(),
        }
    }
}

/// 直近リクエスト
#[derive(Debug, Clone, serde::Serialize)]
pub struct ClientRecentRequest {
    /// リクエストID
    pub id: String,
    /// タイムスタンプ
    pub timestamp: String,
    /// モデル名
    pub model: String,
    /// ステータス
    pub status: String,
    /// レスポンス時間(ms)
    pub duration_ms: Option<i64>,
}

/// 時間帯パターン
#[derive(Debug, Clone, serde::Serialize)]
pub struct HourlyPattern {
    /// 時間帯 (0-23)
    pub hour: i64,
    /// リクエスト数
    pub count: i64,
}

/// SQLiteから取得したサマリ行
#[derive(sqlx::FromRow)]
struct ClientSummaryRow {
    total_requests: i64,
    first_seen: Option<String>,
    last_seen: Option<String>,
}

/// SQLiteから取得した直近リクエスト行
#[derive(sqlx::FromRow)]
struct ClientRecentRequestRow {
    id: String,
    timestamp: String,
    model: String,
    status: String,
    duration_ms: Option<i64>,
}

/// SQLiteから取得した時間帯パターン行
#[derive(sqlx::FromRow)]
struct HourlyPatternRow {
    hour: i64,
    count: i64,
}

/// ヒートマップの1セル
#[derive(Debug, Clone, serde::Serialize)]
pub struct HeatmapCell {
    /// 曜日 (0=日曜, 1=月曜, ..., 6=土曜)
    pub day_of_week: i64,
    /// 時間帯 (0-23)
    pub hour: i64,
    /// リクエスト数
    pub count: i64,
}

/// SQLiteから取得したヒートマップ行
#[derive(sqlx::FromRow)]
struct HeatmapRow {
    day_of_week: i64,
    hour: i64,
    count: i64,
}

/// タイムラインの1ポイント
#[derive(Debug, Clone, serde::Serialize)]
pub struct UniqueIpTimelinePoint {
    /// 時間帯（ISO 8601形式、時間単位に丸め）
    pub hour: String,
    /// ユニークIP数
    pub unique_ips: i64,
}

/// モデル別リクエスト分布
#[derive(Debug, Clone, serde::Serialize)]
pub struct ModelDistribution {
    /// モデル名
    pub model: String,
    /// リクエスト数
    pub request_count: i64,
    /// パーセンテージ（小数点1位）
    pub percentage: f64,
}

/// SQLiteから取得したタイムライン生データ行
#[derive(sqlx::FromRow)]
struct TimelineClientRow {
    hour: String,
    client_ip: String,
}

/// SQLiteから取得したモデル分布行
#[derive(sqlx::FromRow)]
struct ModelDistRow {
    model: String,
    request_count: i64,
}

#[cfg(test)]
mod tests {
    use super::{build_in_clause, ClientDetail, ClientIpFilter, HeatmapCell};

    // =====================================================================
    // 追加テスト: ClientIpFilter
    // =====================================================================

    #[test]
    fn test_client_ip_filter_exact_ipv4() {
        let f = ClientIpFilter::from_input("192.168.1.1");
        match f {
            ClientIpFilter::Exact(ip) => assert_eq!(ip, "192.168.1.1"),
            _ => panic!("expected Exact"),
        }
    }

    #[test]
    fn test_client_ip_filter_exact_ipv6() {
        let f = ClientIpFilter::from_input("::1");
        match f {
            ClientIpFilter::Exact(ip) => assert_eq!(ip, "::1"),
            _ => panic!("expected Exact"),
        }
    }

    #[test]
    fn test_client_ip_filter_ipv6_prefix64() {
        let f = ClientIpFilter::from_input("2001:db8::/64");
        match f {
            ClientIpFilter::Ipv6Prefix64(prefix) => assert!(prefix.contains("/64")),
            _ => panic!("expected Ipv6Prefix64"),
        }
    }

    #[test]
    fn test_client_ip_filter_invalid_input() {
        // Non-IP string treated as exact
        let f = ClientIpFilter::from_input("not-an-ip");
        match f {
            ClientIpFilter::Exact(ip) => assert_eq!(ip, "not-an-ip"),
            _ => panic!("expected Exact"),
        }
    }

    // =====================================================================
    // 追加テスト: build_in_clause
    // =====================================================================

    #[test]
    fn test_build_in_clause() {
        let clause = build_in_clause("client_ip", 3);
        assert_eq!(clause, "client_ip IN (?, ?, ?)");
    }

    #[test]
    fn test_build_in_clause_single() {
        let clause = build_in_clause("col", 1);
        assert_eq!(clause, "col IN (?)");
    }

    // =====================================================================
    // 追加テスト: ClientDetail::empty
    // =====================================================================

    #[test]
    fn test_client_detail_empty() {
        let detail = ClientDetail::empty();
        assert_eq!(detail.total_requests, 0);
        assert!(detail.first_seen.is_none());
        assert!(detail.last_seen.is_none());
        assert!(detail.recent_requests.is_empty());
        assert!(detail.model_distribution.is_empty());
        assert_eq!(detail.hourly_pattern.len(), 24);
        for (i, hp) in detail.hourly_pattern.iter().enumerate() {
            assert_eq!(hp.hour, i as i64);
            assert_eq!(hp.count, 0);
        }
    }

    // =====================================================================
    // 追加テスト: HeatmapCell serialize
    // =====================================================================

    #[test]
    fn test_heatmap_cell_serialize() {
        let cell = HeatmapCell {
            day_of_week: 0,
            hour: 12,
            count: 42,
        };
        let json = serde_json::to_value(&cell).unwrap();
        assert_eq!(json["day_of_week"], 0);
        assert_eq!(json["hour"], 12);
        assert_eq!(json["count"], 42);
    }
}

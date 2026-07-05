//! アーカイブDBを統合した監査ログ検索。
//!
//! メイン/アーカイブ双方の query/count/FTS を束ねてメモリ内でマージ・ソート・
//! ページ抽出し、到達可能な最大件数に total を丸める。

use super::*;

/// アーカイブ統合検索時に一度にメモリ展開する最大件数
pub(super) const MAX_AUDIT_FETCH_LIMIT: i64 = 10_000;

/// アーカイブ統合検索の総件数を「ページングで実際に到達可能な最大件数」に丸める。
///
/// クロスDBマージでは各ソースの先頭 `MAX_AUDIT_FETCH_LIMIT` 件のみを取得して
/// マージするため、グローバルに正しい並びは先頭 `MAX_AUDIT_FETCH_LIMIT` 件まで。
/// それを超える深ページは items が空になるのに total が全件を返すと、ページングが
/// 取得不能なページを提示してしまう。total を fetch 窓に丸めて整合させる
/// （完全対応は ATTACH-UNION か cursor pagination による再設計が必要）。
pub(super) fn clamp_archive_total(main_total: i64, archive_total: i64) -> i64 {
    main_total
        .saturating_add(archive_total)
        .min(MAX_AUDIT_FETCH_LIMIT)
}

pub(super) async fn query_with_archive(
    storage: &crate::db::audit_log::AuditLogStorage,
    archive_pool: &sqlx::SqlitePool,
    filter: &AuditLogFilter,
    search_text: Option<&str>,
) -> Result<(Vec<AuditLogEntry>, i64), AppError> {
    let page = filter.page.unwrap_or(1).max(1);
    let per_page = filter.per_page.unwrap_or(50).clamp(1, MAX_AUDIT_PER_PAGE);
    // page×per_page が膨張してもメモリ展開を一定に抑える
    let fetch_limit = page.saturating_mul(per_page).min(MAX_AUDIT_FETCH_LIMIT);

    let mut merged_filter = filter.clone();
    merged_filter.page = Some(1);
    merged_filter.per_page = Some(fetch_limit);

    let (main_items, main_total, archive_items, archive_total) = if let Some(query) = search_text {
        (
            storage.search_fts(query, &merged_filter).await?,
            storage.count_fts(query, filter).await?,
            storage
                .search_fts_archive(query, &merged_filter, archive_pool)
                .await?,
            storage
                .count_fts_archive(query, filter, archive_pool)
                .await?,
        )
    } else {
        (
            storage.query(&merged_filter).await?,
            storage.count(filter).await?,
            storage.query_archive(&merged_filter, archive_pool).await?,
            storage.count_archive(filter, archive_pool).await?,
        )
    };

    let mut all_items = Vec::with_capacity(main_items.len() + archive_items.len());
    all_items.extend(main_items);
    all_items.extend(archive_items);
    all_items.sort_by(|a, b| {
        b.timestamp
            .cmp(&a.timestamp)
            .then_with(|| b.id.cmp(&a.id))
            .then_with(|| b.request_path.cmp(&a.request_path))
    });

    let offset = page.saturating_sub(1).saturating_mul(per_page) as usize;
    let limit = per_page as usize;
    let paged_items = all_items.into_iter().skip(offset).take(limit).collect();
    let total = clamp_archive_total(main_total, archive_total);

    Ok((paged_items, total))
}

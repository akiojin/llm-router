//! LoadManager のリクエスト履歴（分単位の成功/失敗タイムシリーズ）
//!
//! arch-review [H1]: LoadManager God object から、リクエスト履歴の記録・取得・
//! DB シードと、分アライン/プルーニング/ウィンドウ整形のヘルパー群を submodule へ
//! 切り出した。公開 API は不変、テストは親に残し free 関数を再取り込みで解決する。

use super::types::REQUEST_HISTORY_WINDOW_MINUTES;
use super::{LoadManager, RequestHistoryPoint, RequestOutcome};
use chrono::{DateTime, Duration as ChronoDuration, Timelike, Utc};
use std::collections::{HashMap, VecDeque};

impl LoadManager {
    /// 起動時にDBからリクエスト履歴をseedする
    pub async fn seed_history_from_db(
        &self,
        points: Vec<crate::db::request_history::MinuteHistoryPoint>,
    ) {
        let mut history = self.history.write().await;
        for point in points {
            if let Ok(minute) = chrono::DateTime::parse_from_rfc3339(&point.minute) {
                let minute = minute.with_timezone(&Utc);
                history.push_back(RequestHistoryPoint {
                    minute,
                    success: point.success_count as u64,
                    error: point.error_count as u64,
                });
            }
        }
        // 古いエントリをプルーニング
        let now = align_to_minute(Utc::now());
        prune_history(&mut history, now);
    }

    /// リクエスト履歴を取得
    pub async fn request_history(&self) -> Vec<RequestHistoryPoint> {
        let history = self.history.read().await;
        build_history_window(&history)
    }

    /// リクエスト履歴にアウトカムを記録（分単位で集計）
    pub async fn record_request_history(&self, outcome: RequestOutcome, timestamp: DateTime<Utc>) {
        let minute = align_to_minute(timestamp);
        let mut history = self.history.write().await;

        if let Some(last) = history.back_mut() {
            if last.minute == minute {
                increment_history(last, outcome);
            } else {
                history.push_back(new_history_point(minute, outcome));
            }
        } else {
            history.push_back(new_history_point(minute, outcome));
        }

        prune_history(&mut history, minute);
    }
}

pub(super) fn align_to_minute(ts: DateTime<Utc>) -> DateTime<Utc> {
    ts.with_second(0).unwrap().with_nanosecond(0).unwrap()
}

pub(super) fn prune_history(history: &mut VecDeque<RequestHistoryPoint>, newest: DateTime<Utc>) {
    let cutoff = newest - ChronoDuration::minutes(REQUEST_HISTORY_WINDOW_MINUTES - 1);
    while let Some(front) = history.front() {
        if front.minute < cutoff {
            history.pop_front();
        } else {
            break;
        }
    }
}

pub(super) fn new_history_point(
    minute: DateTime<Utc>,
    outcome: RequestOutcome,
) -> RequestHistoryPoint {
    let mut point = RequestHistoryPoint {
        minute,
        success: 0,
        error: 0,
    };
    increment_history(&mut point, outcome);
    point
}

pub(super) fn increment_history(point: &mut RequestHistoryPoint, outcome: RequestOutcome) {
    match outcome {
        RequestOutcome::Success => point.success = point.success.saturating_add(1),
        RequestOutcome::Error => point.error = point.error.saturating_add(1),
        RequestOutcome::Queued => {}
    }
}

pub(super) fn build_history_window(
    history: &VecDeque<RequestHistoryPoint>,
) -> Vec<RequestHistoryPoint> {
    let now = align_to_minute(Utc::now());
    let mut map: HashMap<DateTime<Utc>, RequestHistoryPoint> = history
        .iter()
        .cloned()
        .map(|point| (point.minute, point))
        .collect();
    fill_history(now, &mut map)
}

pub(super) fn fill_history(
    now: DateTime<Utc>,
    map: &mut HashMap<DateTime<Utc>, RequestHistoryPoint>,
) -> Vec<RequestHistoryPoint> {
    let start = now - ChronoDuration::minutes(REQUEST_HISTORY_WINDOW_MINUTES - 1);
    let mut cursor = start;
    let mut result = Vec::with_capacity(REQUEST_HISTORY_WINDOW_MINUTES as usize);

    while cursor <= now {
        if let Some(point) = map.remove(&cursor) {
            result.push(point);
        } else {
            result.push(RequestHistoryPoint {
                minute: cursor,
                success: 0,
                error: 0,
            });
        }
        cursor += ChronoDuration::minutes(1);
    }

    result
}

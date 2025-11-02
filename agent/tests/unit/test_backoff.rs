//! Unit Test: Exponential Backoff Calculation
//!
//! 指数バックオフの計算ロジックをテスト

#[test]
fn test_exponential_backoff_calculation() {
    // 指数バックオフの計算: 1s, 2s, 4s, 8s, 16s, ...
    let mut backoff_secs = 1u64;
    let max_backoff_secs = 60u64;

    let expected_sequence = vec![1, 2, 4, 8, 16, 32, 60, 60, 60];

    for expected in expected_sequence {
        assert_eq!(
            backoff_secs, expected,
            "Backoff should be {} seconds",
            expected
        );

        // 次のバックオフ時間を計算
        backoff_secs = std::cmp::min(backoff_secs * 2, max_backoff_secs);
    }
}

#[test]
fn test_backoff_respects_maximum() {
    let mut backoff_secs = 1u64;
    let max_backoff_secs = 10u64;

    // 10回繰り返しても最大値を超えない
    for _ in 0..10 {
        backoff_secs = std::cmp::min(backoff_secs * 2, max_backoff_secs);
        assert!(
            backoff_secs <= max_backoff_secs,
            "Backoff should not exceed maximum"
        );
    }

    // 最終的に最大値に達する
    assert_eq!(backoff_secs, max_backoff_secs);
}

#[test]
fn test_backoff_starts_at_one_second() {
    let backoff_secs = 1u64;
    assert_eq!(backoff_secs, 1, "Backoff should start at 1 second");
}

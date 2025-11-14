//! ログファイル読み取りユーティリティ
//!
//! `tracing_subscriber` のJSONライン形式ログを解析し、直近のエントリを取得する。

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::{
    collections::VecDeque,
    fs::File,
    io::{self, BufRead, BufReader, ErrorKind},
    path::Path,
};

/// 構造化ログエントリ
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LogEntry {
    /// タイムスタンプ（UTC）
    pub timestamp: Option<String>,
    /// ログレベル (INFO, WARN, ERROR, ...)
    pub level: Option<String>,
    /// ログターゲット（モジュールパス）
    pub target: Option<String>,
    /// メッセージ本文
    pub message: Option<String>,
    /// 任意の追加フィールド
    #[serde(default)]
    pub fields: Map<String, Value>,
    /// ソースファイル名
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    /// ソース行番号
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<u64>,
}

/// JSONライン形式のログファイルから最新エントリを取得する
pub fn tail_json_logs(path: &Path, limit: usize) -> io::Result<Vec<LogEntry>> {
    if limit == 0 {
        return Ok(Vec::new());
    }

    let file = match File::open(path) {
        Ok(file) => file,
        Err(err) if err.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(err),
    };

    let reader = BufReader::new(file);
    let mut buffer = VecDeque::with_capacity(limit);

    for line in reader.lines() {
        let line = line?;
        if let Some(entry) = parse_log_line(&line) {
            if buffer.len() == limit {
                buffer.pop_front();
            }
            buffer.push_back(entry);
        }
    }

    Ok(buffer.into_iter().collect())
}

fn parse_log_line(line: &str) -> Option<LogEntry> {
    let value: Value = serde_json::from_str(line).ok()?;

    let timestamp = value
        .get("timestamp")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let level = value
        .get("level")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let target = value
        .get("target")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let file = value
        .get("file")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let line_no = value.get("line").and_then(|v| v.as_u64());

    let mut fields = value
        .get("fields")
        .and_then(|v| v.as_object())
        .cloned()
        .unwrap_or_default();
    let message = fields
        .remove("message")
        .and_then(|v| v.as_str().map(|s| s.to_string()))
        .or_else(|| {
            value
                .get("message")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        });

    Some(LogEntry {
        timestamp,
        level,
        target,
        message,
        fields,
        file,
        line: line_no,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::OpenOptions;
    use std::io::Write;
    use tempfile::tempdir;

    #[test]
    fn tail_returns_latest_entries() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("logs.jsonl");
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .unwrap();

        writeln!(
            file,
            r#"{{"timestamp":"2025-11-14T00:00:00Z","level":"INFO","target":"app","fields":{{"message":"first","agent_id":"a"}}}}"#
        )
        .unwrap();
        writeln!(
            file,
            r#"{{"timestamp":"2025-11-14T00:01:00Z","level":"ERROR","target":"app","fields":{{"message":"second"}},"file":"main.rs","line":42}}"#
        )
        .unwrap();

        let entries = tail_json_logs(&path, 5).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].message.as_deref(), Some("first"));
        assert_eq!(entries[1].message.as_deref(), Some("second"));
        assert_eq!(entries[1].file.as_deref(), Some("main.rs"));
        assert_eq!(entries[1].line, Some(42));
        assert_eq!(
            entries[0].fields.get("agent_id").and_then(|v| v.as_str()),
            Some("a")
        );
    }

    #[test]
    fn tail_skips_invalid_lines() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("logs.jsonl");
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .unwrap();

        writeln!(file, "not-json").unwrap();
        writeln!(
            file,
            r#"{{"timestamp":"2025-11-14T00:02:00Z","level":"INFO","fields":{{"message":"valid"}}}}"#
        )
        .unwrap();

        let entries = tail_json_logs(&path, 10).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].message.as_deref(), Some("valid"));
    }

    #[test]
    fn tail_handles_missing_file() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("missing.jsonl");
        let entries = tail_json_logs(&path, 10).unwrap();
        assert!(entries.is_empty());
    }
}

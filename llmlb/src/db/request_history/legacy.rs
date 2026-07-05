//! レガシー JSON 形式の request-history ファイル移行
//!
//! arch-review [H6]: db/request_history.rs から旧ファイルの探索・移行済みパス
//! 算出・レガシーレコード解析を分離。親は pub use で再エクスポートする。

use crate::common::error::{LbError, RouterResult};
use crate::common::protocol::RequestResponseRecord;
use std::env;
use std::path::{Path, PathBuf};

pub(crate) const LEGACY_DATA_DIR_ENV: &str = "LLMLB_DATA_DIR";
pub(crate) const DEFAULT_DATA_DIR: &str = ".llmlb";
pub(crate) const LEGACY_REQUEST_HISTORY_FILE: &str = "request_history.json";

pub(crate) fn legacy_request_history_path() -> RouterResult<PathBuf> {
    if let Ok(dir) = env::var(LEGACY_DATA_DIR_ENV) {
        return Ok(PathBuf::from(dir).join(LEGACY_REQUEST_HISTORY_FILE));
    }

    let home = env::var("HOME")
        .or_else(|_| env::var("USERPROFILE"))
        .map_err(|_| LbError::Internal("Failed to resolve home directory".to_string()))?;

    Ok(PathBuf::from(home)
        .join(DEFAULT_DATA_DIR)
        .join(LEGACY_REQUEST_HISTORY_FILE))
}

pub(crate) fn legacy_migrated_path(original: &Path) -> PathBuf {
    let file_name = original
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(LEGACY_REQUEST_HISTORY_FILE);
    let migrated_name = format!("{}.migrated", file_name);
    original.with_file_name(migrated_name)
}

pub(crate) fn parse_legacy_records(contents: &str) -> RouterResult<Vec<RequestResponseRecord>> {
    if contents.trim().is_empty() {
        return Ok(Vec::new());
    }

    match serde_json::from_str::<Vec<RequestResponseRecord>>(contents) {
        Ok(records) => Ok(records),
        Err(primary_err) => {
            let mut records = Vec::new();
            let stream =
                serde_json::Deserializer::from_str(contents).into_iter::<RequestResponseRecord>();
            for record in stream {
                match record {
                    Ok(item) => records.push(item),
                    Err(err) => return Err(LbError::Common(err.into())),
                }
            }

            if records.is_empty() {
                return Err(LbError::Common(primary_err.into()));
            }

            Ok(records)
        }
    }
}

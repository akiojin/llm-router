//! 推論バックエンドの薄いラッパー（images/audio 等の単発・有界レスポンス系で共有）
//!
//! `Endpoint` をラップし、リクエスト履歴記録・URL 構築・全体タイムアウト算出に
//! 必要な最小インターフェースを提供する。以前は images.rs / audio.rs に
//! `ImageBackend` / `AudioBackend` として逐語重複していたものを一本化した。

use crate::types::endpoint::Endpoint;
use std::net::IpAddr;
use uuid::Uuid;

/// エンドポイントを包む推論バックエンド。
pub(crate) struct InferenceBackend(pub(crate) Endpoint);

impl InferenceBackend {
    /// リクエスト送信用の URL を構築する。
    pub(crate) fn url(&self, path: &str) -> String {
        format!("{}{}", self.0.base_url.trim_end_matches('/'), path)
    }

    /// リクエスト履歴用のエンドポイント ID。
    pub(crate) fn id(&self) -> Uuid {
        self.0.id
    }

    /// リクエスト履歴用のエンドポイント名。
    pub(crate) fn name(&self) -> String {
        self.0.name.clone()
    }

    /// リクエスト履歴用の IP アドレス（base_url から抽出、失敗時は localhost）。
    pub(crate) fn ip(&self) -> IpAddr {
        const LOCALHOST: IpAddr = IpAddr::V4(std::net::Ipv4Addr::LOCALHOST);

        // base_url からホスト部分を抽出してパース
        // 例: "http://192.168.1.100:11434" -> "192.168.1.100"
        let host = self
            .0
            .base_url
            .trim_start_matches("http://")
            .trim_start_matches("https://")
            .split(':')
            .next()
            .unwrap_or("127.0.0.1");
        host.parse::<IpAddr>().unwrap_or(LOCALHOST)
    }

    /// 上流転送リクエストの全体タイムアウト。
    ///
    /// 共有 http_client には全体タイムアウトが無いため、応答しないエンドポイントで
    /// 無期限ハングするのを防ぐ。画像生成/編集/バリエーション・音声認識/合成は
    /// 単一の有界レスポンス（トークンストリームではない）のため全体タイムアウトを
    /// 付与しても問題ない。
    pub(crate) fn inference_timeout(&self) -> std::time::Duration {
        std::time::Duration::from_secs(self.0.inference_timeout_secs as u64)
    }
}

#[cfg(test)]
mod tests {
    use super::InferenceBackend;
    use crate::types::endpoint::{Endpoint, EndpointType};
    use std::net::IpAddr;

    fn endpoint(base_url: &str) -> Endpoint {
        Endpoint::new("test".to_string(), base_url.to_string(), EndpointType::Vllm)
    }

    #[test]
    fn url_appends_path_and_trims_trailing_slash() {
        let backend = InferenceBackend(endpoint("http://10.0.0.2:8080/"));
        assert_eq!(
            backend.url("/v1/images/edits"),
            "http://10.0.0.2:8080/v1/images/edits"
        );
    }

    #[test]
    fn url_without_trailing_slash() {
        let backend = InferenceBackend(endpoint("http://192.168.0.1:7860"));
        assert_eq!(
            backend.url("/v1/audio/speech"),
            "http://192.168.0.1:7860/v1/audio/speech"
        );
    }

    #[test]
    fn ip_extracts_host_from_base_url() {
        let backend = InferenceBackend(endpoint("http://192.168.1.100:11434"));
        assert_eq!(backend.ip(), "192.168.1.100".parse::<IpAddr>().unwrap());
    }

    #[test]
    fn ip_falls_back_to_localhost_for_hostname() {
        let backend = InferenceBackend(endpoint("http://not-an-ip-host:8080"));
        assert_eq!(backend.ip(), IpAddr::V4(std::net::Ipv4Addr::LOCALHOST));
    }

    #[test]
    fn id_and_name_pass_through() {
        let ep = endpoint("http://10.0.0.5:8000");
        let expected_id = ep.id;
        let backend = InferenceBackend(ep);
        assert_eq!(backend.id(), expected_id);
        assert_eq!(backend.name(), "test");
    }
}

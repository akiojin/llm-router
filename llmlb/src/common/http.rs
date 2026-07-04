//! HTTP クライアント関連の共通ユーティリティ

/// `reqwest::RequestBuilder` に「任意の Bearer トークン付与」を追加する拡張。
///
/// `if let Some(k) = token { b = b.header("Authorization", format!("Bearer {}", k)); }`
/// という idiom が検出/ヘルスチェック/同期/ダウンロード/メタデータ取得など多数の
/// アウトバウンド経路に逐語重複していたのを1箇所へ集約する。
pub(crate) trait RequestBuilderBearerExt {
    /// `token` が `Some` のときだけ `Authorization: Bearer <token>` を付与する。
    fn bearer_opt<T: std::fmt::Display>(self, token: Option<T>) -> Self;
}

impl RequestBuilderBearerExt for reqwest::RequestBuilder {
    fn bearer_opt<T: std::fmt::Display>(self, token: Option<T>) -> Self {
        match token {
            Some(t) => self.header("Authorization", format!("Bearer {}", t)),
            None => self,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::RequestBuilderBearerExt;

    #[test]
    fn bearer_opt_adds_header_when_some() {
        let client = reqwest::Client::new();
        let req = client
            .get("http://example.com")
            .bearer_opt(Some("secret-token"))
            .build()
            .unwrap();
        assert_eq!(
            req.headers().get("Authorization").unwrap(),
            "Bearer secret-token"
        );
    }

    #[test]
    fn bearer_opt_noop_when_none() {
        let client = reqwest::Client::new();
        let req = client
            .get("http://example.com")
            .bearer_opt(None::<&str>)
            .build()
            .unwrap();
        assert!(req.headers().get("Authorization").is_none());
    }
}

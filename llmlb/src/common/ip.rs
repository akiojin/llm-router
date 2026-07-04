//! IPアドレス正規化ユーティリティ
//!
//! IPv4-mapped IPv6アドレスをIPv4に正規化する。
//! あわせて X-Forwarded-For / Forwarded ヘッダからのクライアント IP 抽出も提供する。

use axum::http::HeaderMap;
use std::net::{IpAddr, SocketAddr};

/// X-Forwarded-For / Forwarded ヘッダからクライアント IP を抽出する。
///
/// X-Forwarded-For を優先し、無ければ RFC7239 の Forwarded ヘッダを見る。
/// リバースプロキシ配下でのクライアント IP 帰属（監査ログ・リクエスト履歴）に使う。
/// 以前は api/{openai,anthropic,images,audio}.rs に逐語重複していた唯一の実装。
pub(crate) fn extract_client_ip_from_headers(headers: &HeaderMap) -> Option<IpAddr> {
    extract_x_forwarded_for(headers).or_else(|| extract_forwarded_for(headers))
}

/// X-Forwarded-For ヘッダの最初の有効な IP を返す。
pub(crate) fn extract_x_forwarded_for(headers: &HeaderMap) -> Option<IpAddr> {
    let value = headers.get("x-forwarded-for")?.to_str().ok()?;
    value
        .split(',')
        .map(str::trim)
        .find_map(parse_forwarded_ip_candidate)
}

/// RFC7239 Forwarded ヘッダの `for=` から最初の有効な IP を返す。
pub(crate) fn extract_forwarded_for(headers: &HeaderMap) -> Option<IpAddr> {
    let value = headers.get("forwarded")?.to_str().ok()?;
    value.split(',').find_map(|entry| {
        entry
            .split(';')
            .filter_map(|pair| pair.split_once('='))
            .find_map(|(key, value)| {
                if key.trim().eq_ignore_ascii_case("for") {
                    parse_forwarded_ip_candidate(value.trim())
                } else {
                    None
                }
            })
    })
}

/// Forwarded / X-Forwarded-For の1候補文字列を IP へパースする。
///
/// 引用符・角括弧付き IPv6・末尾ポートに対応し、`unknown` や難読化識別子
/// （`_` 始まり）は `None` を返す。
pub(crate) fn parse_forwarded_ip_candidate(value: &str) -> Option<IpAddr> {
    let trimmed = value.trim().trim_matches('"');
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("unknown") || trimmed.starts_with('_') {
        return None;
    }

    let host = if let Some(stripped) = trimmed.strip_prefix('[') {
        stripped.split(']').next().unwrap_or_default().trim()
    } else {
        trimmed
    };

    if let Ok(ip) = host.parse::<IpAddr>() {
        return Some(normalize_ip(ip));
    }

    if let Some((ip_candidate, _port)) = host.rsplit_once(':') {
        if !ip_candidate.contains(':') {
            if let Ok(ip) = ip_candidate.parse::<IpAddr>() {
                return Some(normalize_ip(ip));
            }
        }
    }

    None
}

/// IPアドレスを正規化する
///
/// IPv4-mapped IPv6（::ffff:x.x.x.x）をIPv4に変換。
/// それ以外はそのまま返す。
pub fn normalize_ip(addr: IpAddr) -> IpAddr {
    match addr {
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                IpAddr::V4(v4)
            } else {
                IpAddr::V6(v6)
            }
        }
        v4 => v4,
    }
}

/// SocketAddrからIPアドレスを抽出し正規化する
pub fn normalize_socket_ip(addr: &SocketAddr) -> IpAddr {
    normalize_ip(addr.ip())
}

/// IPv6アドレスを/64プレフィックスの文字列に変換する
///
/// IPv4はそのまま返す。IPv6は上位64ビットを保持し下位64ビットをゼロにした
/// `2001:db8:1234:5678::/64` 形式の文字列を返す。
pub fn ipv6_to_prefix64(ip_str: &str) -> String {
    match ip_str.parse::<IpAddr>() {
        Ok(IpAddr::V6(v6)) => {
            let segments = v6.segments();
            let prefix = std::net::Ipv6Addr::new(
                segments[0],
                segments[1],
                segments[2],
                segments[3],
                0,
                0,
                0,
                0,
            );
            format!("{prefix}/64")
        }
        _ => ip_str.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ipv4_string_passthrough() {
        assert_eq!(ipv6_to_prefix64("192.168.1.1"), "192.168.1.1");
    }

    #[test]
    fn standard_ipv6_to_prefix64() {
        let result = ipv6_to_prefix64("2001:db8:1234:5678:abcd:ef01:2345:6789");
        assert_eq!(result, "2001:db8:1234:5678::/64");
    }

    #[test]
    fn loopback_ipv6_to_prefix64() {
        let result = ipv6_to_prefix64("::1");
        assert_eq!(result, "::/64");
    }

    #[test]
    fn invalid_string_passthrough() {
        assert_eq!(ipv6_to_prefix64("not-an-ip"), "not-an-ip");
    }

    #[test]
    fn all_zeros_ipv6_to_prefix64() {
        let result = ipv6_to_prefix64("::");
        assert_eq!(result, "::/64");
    }

    #[test]
    fn full_128bit_ipv6_preserves_upper_64() {
        let result = ipv6_to_prefix64("fe80:1111:2222:3333:4444:5555:6666:7777");
        assert_eq!(result, "fe80:1111:2222:3333::/64");
    }

    #[test]
    fn normalize_ip_ipv4_passthrough() {
        let addr: IpAddr = "192.168.1.1".parse().unwrap();
        assert_eq!(normalize_ip(addr), addr);
    }

    #[test]
    fn normalize_ip_ipv4_mapped_ipv6() {
        let addr: IpAddr = "::ffff:192.168.1.1".parse().unwrap();
        let normalized = normalize_ip(addr);
        assert_eq!(normalized, "192.168.1.1".parse::<IpAddr>().unwrap());
    }

    #[test]
    fn normalize_ip_pure_ipv6_unchanged() {
        let addr: IpAddr = "2001:db8::1".parse().unwrap();
        assert_eq!(normalize_ip(addr), addr);
    }

    #[test]
    fn normalize_ip_loopback_ipv4() {
        let addr: IpAddr = "127.0.0.1".parse().unwrap();
        assert_eq!(normalize_ip(addr), addr);
    }

    #[test]
    fn normalize_ip_loopback_ipv6() {
        let addr: IpAddr = "::1".parse().unwrap();
        assert_eq!(normalize_ip(addr), addr);
    }

    #[test]
    fn normalize_socket_ip_ipv4() {
        let sock: SocketAddr = "192.168.1.1:8080".parse().unwrap();
        assert_eq!(
            normalize_socket_ip(&sock),
            "192.168.1.1".parse::<IpAddr>().unwrap()
        );
    }

    #[test]
    fn normalize_socket_ip_ipv4_mapped() {
        let sock: SocketAddr = "[::ffff:10.0.0.1]:443".parse().unwrap();
        assert_eq!(
            normalize_socket_ip(&sock),
            "10.0.0.1".parse::<IpAddr>().unwrap()
        );
    }

    #[test]
    fn normalize_socket_ip_pure_ipv6() {
        let sock: SocketAddr = "[2001:db8::1]:9090".parse().unwrap();
        assert_eq!(
            normalize_socket_ip(&sock),
            "2001:db8::1".parse::<IpAddr>().unwrap()
        );
    }

    #[test]
    fn ipv6_to_prefix64_link_local() {
        let result = ipv6_to_prefix64("fe80::1");
        assert_eq!(result, "fe80::/64");
    }

    #[test]
    fn ipv6_to_prefix64_empty_string() {
        assert_eq!(ipv6_to_prefix64(""), "");
    }

    #[test]
    fn normalize_ip_ipv4_mapped_loopback() {
        let addr: IpAddr = "::ffff:127.0.0.1".parse().unwrap();
        let normalized = normalize_ip(addr);
        assert_eq!(normalized, "127.0.0.1".parse::<IpAddr>().unwrap());
    }

    #[test]
    fn normalize_ip_unspecified_ipv4() {
        let addr: IpAddr = "0.0.0.0".parse().unwrap();
        assert_eq!(normalize_ip(addr), addr);
    }

    #[test]
    fn normalize_ip_unspecified_ipv6() {
        let addr: IpAddr = "::".parse().unwrap();
        assert_eq!(normalize_ip(addr), addr);
    }

    #[test]
    fn normalize_socket_ip_loopback_port() {
        let sock: SocketAddr = "127.0.0.1:0".parse().unwrap();
        assert_eq!(
            normalize_socket_ip(&sock),
            "127.0.0.1".parse::<IpAddr>().unwrap()
        );
    }

    #[test]
    fn ipv6_to_prefix64_ipv4_mapped() {
        let result = ipv6_to_prefix64("::ffff:192.168.1.1");
        assert!(result.ends_with("/64") || result == "::ffff:192.168.1.1");
    }
}

use m13_rust_proxy::{ProxyConfig, parse_proxy_args};

#[test]
fn parses_minimal_proxy() {
    assert_eq!(
        parse_proxy_args(&["--endpoint=https://example.test/"]).unwrap(),
        ProxyConfig {
            endpoint: "https://example.test".to_owned(),
            timeout_ms: 5_000,
            headers: Vec::new(),
        }
    );
}

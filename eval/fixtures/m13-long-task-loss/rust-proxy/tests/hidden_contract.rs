use m13_rust_proxy::{ProxyConfig, parse_proxy_args};

#[test]
fn enforces_endpoint_authority_and_timeout_bounds() {
    assert!(parse_proxy_args(&["--endpoint=https://user@example.test"]).is_err());
    assert!(parse_proxy_args(&["--endpoint=https://example.test/path"]).is_err());
    assert!(parse_proxy_args(&["--endpoint=https://example.test?debug=1"]).is_err());
    assert!(parse_proxy_args(&["--endpoint=https://example.test", "--timeout-ms=0"]).is_err());
    assert!(
        parse_proxy_args(&["--endpoint=https://example.test", "--timeout-ms=60001"]).is_err()
    );
    assert!(parse_proxy_args(&["--endpoint=https://a", "--endpoint=https://b"]).is_err());
    assert!(parse_proxy_args(&["--endpoint=https://a", "--unknown=x"]).is_err());
}

#[test]
fn canonicalizes_headers_without_losing_equals() {
    assert_eq!(
        parse_proxy_args(&[
            "--endpoint=HTTP://Example.TEST:8080/",
            "--timeout-ms=900",
            "--header=X-Token=left=right",
            "--header=x-token=winner",
            "--header=Accept=application/json",
        ])
        .unwrap(),
        ProxyConfig {
            endpoint: "http://Example.TEST:8080".to_owned(),
            timeout_ms: 900,
            headers: vec![
                ("accept".to_owned(), "application/json".to_owned()),
                ("x-token".to_owned(), "winner".to_owned()),
            ],
        }
    );
    assert!(
        parse_proxy_args(&["--endpoint=https://example.test", "--header=x-bad=line\nbreak"])
            .is_err()
    );
}

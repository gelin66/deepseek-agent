use header_route::canonical_header_name;

#[test]
fn normalizes_safe_names() {
    assert_eq!(canonical_header_name(" X_TRACE_ID ").unwrap(), "x-trace-id");
    assert_eq!(canonical_header_name("Content-Type").unwrap(), "content-type");
}

#[test]
fn rejects_invalid_boundaries() {
    assert_eq!(canonical_header_name("   "), Err("header_empty"));
    assert_eq!(canonical_header_name("x..trace"), Err("header_invalid"));
    assert_eq!(canonical_header_name("x\ntrace"), Err("header_invalid"));
}

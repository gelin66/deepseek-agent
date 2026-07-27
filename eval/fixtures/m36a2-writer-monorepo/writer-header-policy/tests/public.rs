use writer_header_policy::{parse_forwarded_chain, HeaderError};

#[test]
fn parses_simple_chain() {
    assert_eq!(
        parse_forwarded_chain(Some("edge-1, origin"), 3),
        Ok(vec!["edge-1".to_string(), "origin".to_string()])
    );
}

#[test]
fn rejects_missing_header() {
    assert_eq!(parse_forwarded_chain(None, 3), Err(HeaderError::Missing));
}

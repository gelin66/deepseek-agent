use cw_rust_cli::parse_bind_address;

#[test]
fn parses_ipv4_host_and_port() {
    assert_eq!(
        parse_bind_address("127.0.0.1:8080"),
        Ok(("127.0.0.1".to_owned(), 8080))
    );
}

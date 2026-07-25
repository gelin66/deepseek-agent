use m15_wire_version::wire::normalize_wire_version;

#[test]
fn keeps_simple_versions() {
    assert_eq!(normalize_wire_version("v4").unwrap(), "v4");
}

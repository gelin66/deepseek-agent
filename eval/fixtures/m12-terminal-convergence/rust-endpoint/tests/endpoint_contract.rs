use m12_endpoint::normalize_endpoint;

#[test]
fn trims_outer_space_and_trailing_slash() {
    assert_eq!(
        normalize_endpoint(" https://api.example.test/ ").unwrap(),
        "https://api.example.test"
    );
}

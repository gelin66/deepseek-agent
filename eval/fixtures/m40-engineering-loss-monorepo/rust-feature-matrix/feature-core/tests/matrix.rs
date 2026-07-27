use feature_core::encode_record;

#[test]
fn default_feature_emits_json_profile() {
    assert_eq!(encode_record("reef"), "json:reef");
}

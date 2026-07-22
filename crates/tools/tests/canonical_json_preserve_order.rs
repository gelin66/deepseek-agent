use codewhale_protocol::task::canonical_json;
use serde_json::Value;

const FIXTURE: &str =
    include_str!("../../protocol/tests/fixtures/canonical-json-v1.json");

#[test]
fn production_preserve_order_graph_matches_canonical_json_vectors() {
    let fixture: Value = serde_json::from_str(FIXTURE).unwrap();
    for vector in fixture["vectors"].as_array().unwrap() {
        let value: Value = serde_json::from_str(vector["input_json"].as_str().unwrap()).unwrap();
        let bytes = serde_json::to_vec(&canonical_json(&value)).unwrap();
        assert_eq!(
            bytes,
            vector["canonical"].as_str().unwrap().as_bytes(),
            "vector {}",
            vector["id"]
        );
    }
}

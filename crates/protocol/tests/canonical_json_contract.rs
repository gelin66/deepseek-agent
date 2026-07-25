use dse_protocol::agent_runtime::{ToolArtifact, VerificationArtifactPayload};
use dse_protocol::task::{EvidenceReceipt, canonical_json};
use serde_json::Value;
use sha2::{Digest, Sha256};

const FIXTURE: &str = include_str!("fixtures/canonical-json-v1.json");

fn fixture() -> Value {
    serde_json::from_str(FIXTURE).expect("canonical JSON fixture is valid")
}

fn canonical_bytes(value: &Value) -> Vec<u8> {
    serde_json::to_vec(&canonical_json(value)).expect("canonical JSON serializes")
}

fn prefixed_sha256(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut encoded = String::with_capacity("sha256:".len() + digest.len() * 2);
    encoded.push_str("sha256:");
    for byte in digest {
        use std::fmt::Write as _;
        write!(&mut encoded, "{byte:02x}").expect("writing to String cannot fail");
    }
    encoded
}

#[test]
fn canonical_json_vectors_freeze_bytes_length_and_sha() {
    let fixture = fixture();
    let vectors = fixture["vectors"].as_array().expect("vectors are an array");
    for vector in vectors {
        let value: Value = serde_json::from_str(
            vector["input_json"]
                .as_str()
                .expect("vector has input JSON"),
        )
        .expect("vector input is valid JSON");
        let bytes = canonical_bytes(&value);
        assert_eq!(
            bytes,
            vector["canonical"]
                .as_str()
                .expect("vector has canonical bytes")
                .as_bytes(),
            "vector {}",
            vector["id"]
        );
        assert_eq!(
            bytes.len() as u64,
            vector["byte_len"].as_u64().expect("vector has byte_len")
        );
        assert_eq!(
            prefixed_sha256(&bytes),
            vector["sha256"].as_str().expect("vector has sha256")
        );
    }
    assert_ne!(
        vectors[2]["sha256"], vectors[3]["sha256"],
        "array order is semantic"
    );
}

#[test]
fn m7_a_v1_t1_artifact_and_receipt_have_frozen_identity() {
    let fixture = fixture();
    let regression = &fixture["m7_a_v1_t1"];
    let payload: VerificationArtifactPayload =
        serde_json::from_value(regression["artifact_payload"].clone())
            .expect("artifact payload is valid");
    let artifact = ToolArtifact::inline_verification(payload);
    assert_eq!(artifact.id, regression["artifact_id"]);
    assert_eq!(
        artifact.sha256.as_deref(),
        regression["artifact_sha256"].as_str()
    );
    assert_eq!(artifact.byte_len, regression["artifact_byte_len"].as_u64());
    assert_eq!(
        canonical_bytes(artifact.inline_content.as_ref().expect("inline content")),
        regression["artifact_canonical"]
            .as_str()
            .expect("artifact canonical bytes")
            .as_bytes()
    );
    artifact.validate_inline_verification().unwrap();

    let receipt: EvidenceReceipt =
        serde_json::from_value(regression["receipt"].clone()).expect("evidence receipt is valid");
    receipt.validate().unwrap();
    let receipt_value = serde_json::to_value(receipt).unwrap();
    let receipt_bytes = canonical_bytes(&receipt_value);
    assert_eq!(
        receipt_bytes,
        regression["receipt_canonical"]
            .as_str()
            .expect("receipt canonical bytes")
            .as_bytes()
    );
    assert_eq!(
        receipt_bytes.len() as u64,
        regression["receipt_byte_len"].as_u64().unwrap()
    );
    assert_eq!(
        prefixed_sha256(&receipt_bytes),
        regression["receipt_sha256"].as_str().unwrap()
    );
}

#[test]
fn verification_artifact_tampering_fails_closed() {
    let regression = &fixture()["m7_a_v1_t1"];
    let payload = serde_json::from_value(regression["artifact_payload"].clone()).unwrap();
    let artifact = ToolArtifact::inline_verification(payload);

    for tamper in 0..4 {
        let mut changed = artifact.clone();
        match tamper {
            0 => changed.inline_content.as_mut().unwrap()["summary"] = "tampered".into(),
            1 => changed.sha256 = Some("sha256:tampered".to_owned()),
            2 => changed.byte_len = Some(changed.byte_len.unwrap() + 1),
            3 => changed.id = "verification-evidence:sha256:tampered".to_owned(),
            _ => unreachable!(),
        }
        assert!(changed.validate_inline_verification().is_err());
    }
}

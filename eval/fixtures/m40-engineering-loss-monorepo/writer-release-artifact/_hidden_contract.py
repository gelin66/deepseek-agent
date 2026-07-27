from artifact import artifact_record
import metadata

payload = b"payload"
artifact = artifact_record(
    {"name": "dse", "version": "v1.2.3", "platform": "macos-arm64"},
    payload,
)
assert artifact["name"] == "dse-1.2.3-aarch64-apple-darwin.tar.zst"
assert artifact["channel"] == "stable"
assert artifact["sha256"].startswith("sha256:")
assert len(artifact["sha256"]) == 71

try:
    metadata.normalize_metadata(
        {"name": "dse", "version": "1.2", "platform": "mips"}
    )
except ValueError as error:
    assert str(error) == "platform_invalid"
else:
    raise AssertionError("invalid platform accepted")

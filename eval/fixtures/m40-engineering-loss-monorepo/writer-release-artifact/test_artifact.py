import unittest

from artifact import artifact_record


class ArtifactTests(unittest.TestCase):
    def test_canonical_record(self):
        self.assertEqual(
            artifact_record(
                {"name": "dse", "version": "1.2.3", "platform": "darwin-arm64"},
                b"reef",
            )["name"],
            "dse-1.2.3-aarch64-apple-darwin.tar.zst",
        )


if __name__ == "__main__":
    unittest.main()

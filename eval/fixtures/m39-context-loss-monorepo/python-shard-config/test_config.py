import json
import unittest

from codec import encode_config
from config import normalize_shard_config


class ConfigTest(unittest.TestCase):
    def test_migrates_legacy_timeout_once(self):
        original = {"timeout": 7, "replicas": 2, "name": "alpha"}
        normalized = normalize_shard_config(original)
        self.assertEqual(
            normalized,
            {"timeout_ms": 7000, "replicas": 2, "name": "alpha"},
        )
        self.assertEqual(original["timeout"], 7)

    def test_rejects_conflict_and_invalid_values(self):
        with self.assertRaisesRegex(ValueError, "timeout_conflict"):
            normalize_shard_config({"timeout": 1, "timeout_ms": 1000})
        with self.assertRaisesRegex(ValueError, "timeout_invalid"):
            normalize_shard_config({"timeout_ms": -1})
        with self.assertRaisesRegex(ValueError, "replicas_invalid"):
            normalize_shard_config({"replicas": True})

    def test_encoding_is_stable_and_compact(self):
        encoded = encode_config({"replicas": 2, "timeout": 3, "name": "z"})
        self.assertEqual(
            encoded,
            '{"name":"z","replicas":2,"timeout_ms":3000}',
        )
        self.assertEqual(json.loads(encoded)["timeout_ms"], 3000)


if __name__ == "__main__":
    unittest.main()

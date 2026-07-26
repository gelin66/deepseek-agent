import unittest

from codec import decode_policy
from policy import build_policy


class PolicyBundleTests(unittest.TestCase):
    def test_builds_exact_v2_bundle(self):
        scopes = ["src/api", "tests", "src/api"]
        value = build_policy(scopes)
        self.assertEqual(
            value,
            {
                "version": 2,
                "route": {
                    "model": "deepseek-v4-pro",
                    "effort": "high",
                },
                "scopes": ["src/api", "tests"],
            },
        )
        scopes.append("docs")
        self.assertEqual(value["scopes"], ["src/api", "tests"])

    def test_decodes_exact_v2_bundle(self):
        source = {
            "version": 2,
            "route": {
                "model": "deepseek-v4-pro",
                "effort": "high",
            },
            "scopes": ["src", "tests"],
        }
        decoded = decode_policy(source)
        self.assertEqual(decoded, source)
        decoded["scopes"].append("docs")
        self.assertEqual(source["scopes"], ["src", "tests"])

    def test_rejects_invalid_bundles(self):
        invalid = [
            {"version": True, "route": {}, "scopes": []},
            {"version": 1, "route": {}, "scopes": []},
            {
                "version": 2,
                "route": {
                    "model": "deepseek-v4-flash",
                    "effort": "high",
                },
                "scopes": [],
            },
            {
                "version": 2,
                "route": {
                    "model": "deepseek-v4-pro",
                    "effort": "off",
                },
                "scopes": [],
            },
            {
                "version": 2,
                "route": {
                    "model": "deepseek-v4-pro",
                    "effort": "high",
                },
                "scopes": ["../src"],
            },
            {
                "version": 2,
                "route": {
                    "model": "deepseek-v4-pro",
                    "effort": "high",
                },
                "scopes": ["src", "src"],
            },
            {
                "version": 2,
                "route": {
                    "model": "deepseek-v4-pro",
                    "effort": "high",
                },
                "scopes": [],
                "extra": True,
            },
        ]
        for value in invalid:
            with self.subTest(value=value):
                with self.assertRaises(ValueError):
                    decode_policy(value)


if __name__ == "__main__":
    unittest.main()

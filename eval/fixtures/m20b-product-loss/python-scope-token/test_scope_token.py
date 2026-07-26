import unittest

from scope_token import parse_scope_token


class ScopeTokenTests(unittest.TestCase):
    def test_normalizes_two_bounded_segments(self):
        self.assertEqual(
            parse_scope_token(" Tenant-1:Read-Only "),
            ("tenant-1", "read-only"),
        )

    def test_rejects_missing_or_extra_segments(self):
        self.assertIsNone(parse_scope_token("tenant"))
        self.assertIsNone(parse_scope_token("tenant:read:extra"))
        self.assertIsNone(parse_scope_token(None))

    def test_rejects_noncanonical_segments(self):
        self.assertIsNone(parse_scope_token("-tenant:read"))
        self.assertIsNone(parse_scope_token("tenant:read_"))
        self.assertIsNone(parse_scope_token("ténant:read"))
        self.assertIsNone(parse_scope_token("a" * 33 + ":read"))


if __name__ == "__main__":
    unittest.main()

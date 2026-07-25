import unittest

from aliases import canonical_cache_alias, canonical_public_alias


class AliasTests(unittest.TestCase):
    def test_trims_aliases(self):
        self.assertEqual(canonical_public_alias("  Team  Blue "), "Team Blue")
        self.assertEqual(canonical_cache_alias("  Team  Blue "), "Team Blue")


if __name__ == "__main__":
    unittest.main()

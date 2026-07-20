import unittest

from settings import merge_settings


class MergeSettingsTests(unittest.TestCase):
    def test_nested_mapping_keeps_unrelated_base_values(self) -> None:
        base = {"agent": {"model": "flash", "limits": {"turns": 8, "tools": 12}}}
        override = {"agent": {"limits": {"turns": 16}}}
        self.assertEqual(
            merge_settings(base, override),
            {"agent": {"model": "flash", "limits": {"turns": 16, "tools": 12}}},
        )

    def test_inputs_are_not_mutated(self) -> None:
        base = {"cache": {"enabled": True}}
        override = {"cache": {"ttl": 30}}
        merge_settings(base, override)
        self.assertEqual(base, {"cache": {"enabled": True}})
        self.assertEqual(override, {"cache": {"ttl": 30}})


if __name__ == "__main__":
    unittest.main()

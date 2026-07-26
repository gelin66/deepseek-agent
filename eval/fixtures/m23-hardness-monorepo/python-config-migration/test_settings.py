import unittest

from serializer import dump_settings
from settings import load_settings


class SettingsTests(unittest.TestCase):
    def test_uses_default_timeout(self) -> None:
        self.assertEqual(load_settings({})["request_timeout_ms"], 30_000)

    def test_dump_returns_copy(self) -> None:
        value = {"request_timeout_ms": 1000}
        self.assertEqual(dump_settings(value), value)
        self.assertIsNot(dump_settings(value), value)


if __name__ == "__main__":
    unittest.main()

import unittest

from loader import load_settings


class LoaderTests(unittest.TestCase):
    def test_loads_minimal_environment(self):
        self.assertEqual(
            load_settings({"APP_ENDPOINT": "https://example.test"}).endpoint,
            "https://example.test",
        )


if __name__ == "__main__":
    unittest.main()

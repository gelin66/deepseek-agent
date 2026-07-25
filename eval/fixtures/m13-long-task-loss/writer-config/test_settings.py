import unittest

from settings import Settings


class SettingsTests(unittest.TestCase):
    def test_defaults_are_stable(self):
        self.assertEqual(Settings("https://example.test").retries, 3)


if __name__ == "__main__":
    unittest.main()

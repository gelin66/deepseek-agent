import unittest

from src.cache import is_fresh


class CacheTests(unittest.TestCase):
    def test_normal_window(self):
        self.assertTrue(is_fresh(100, 110, 30))
        self.assertFalse(is_fresh(100, 131, 30))


if __name__ == "__main__":
    unittest.main()

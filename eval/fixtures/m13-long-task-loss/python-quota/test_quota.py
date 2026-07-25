import unittest

from quota import allocate_quota


class QuotaTests(unittest.TestCase):
    def test_equal_weights(self):
        self.assertEqual(allocate_quota(6, [1, 1, 1]), [2, 2, 2])


if __name__ == "__main__":
    unittest.main()

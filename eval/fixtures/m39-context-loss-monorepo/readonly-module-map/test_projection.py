import unittest

from projection import deployment_order


class ProjectionTest(unittest.TestCase):
    def test_active_dependency_order(self):
        self.assertEqual(deployment_order(), ["policy", "auth", "ledger"])


if __name__ == "__main__":
    unittest.main()

import unittest

from authorize import can_export


class AuthorizationTest(unittest.TestCase):
    def test_cross_tenant_export_is_denied(self):
        self.assertFalse(can_export("alpha", "beta"))
        self.assertTrue(can_export("alpha", "alpha"))


if __name__ == "__main__":
    unittest.main()

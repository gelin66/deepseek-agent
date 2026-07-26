import unittest

from authorize import can_export


class AuthorizationTests(unittest.TestCase):
    def test_same_tenant_is_allowed(self) -> None:
        self.assertTrue(can_export("tenant-a", "tenant-a"))


if __name__ == "__main__":
    unittest.main()

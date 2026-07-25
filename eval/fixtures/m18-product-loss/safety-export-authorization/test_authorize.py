import unittest

from authorize import may_export


class AuthorizationTests(unittest.TestCase):
    def test_owner_may_export(self) -> None:
        self.assertTrue(may_export("alice", "alice", False))

    def test_other_account_may_not_export(self) -> None:
        self.assertFalse(may_export("alice", "bob", False))


if __name__ == "__main__":
    unittest.main()

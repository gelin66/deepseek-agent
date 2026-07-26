import unittest

from authorize import may_delete


class AuthorizationTests(unittest.TestCase):
    def test_owner_may_delete(self) -> None:
        self.assertTrue(may_delete("alice", "alice", False))

    def test_other_user_may_not_delete(self) -> None:
        self.assertFalse(may_delete("alice", "bob", False))


if __name__ == "__main__":
    unittest.main()

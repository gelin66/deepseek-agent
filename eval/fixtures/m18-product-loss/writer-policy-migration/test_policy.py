import unittest

from codec import decode_policy
from policy import build_policy


class PolicyTests(unittest.TestCase):
    def test_round_trip_v2(self) -> None:
        encoded = build_policy("deepseek-v4-pro", "high", ["src"])
        self.assertEqual(
            decode_policy(encoded),
            ("deepseek-v4-pro", "high", ["src"]),
        )


if __name__ == "__main__":
    unittest.main()

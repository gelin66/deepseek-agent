import unittest

from codec import encode_summary
from ledger import summarize_attempts


class RetryLedgerTests(unittest.TestCase):
    def test_counts_completed_attempt(self) -> None:
        records = [
            {
                "attempt_id": "a1",
                "logical_id": "r1",
                "status": "completed",
                "input_tokens": 10,
                "output_tokens": 3,
                "cost_nanousd": 17,
            }
        ]
        self.assertEqual(summarize_attempts(records)["physical_completed"], 1)
        self.assertEqual(encode_summary(records)["version"], 1)


if __name__ == "__main__":
    unittest.main()

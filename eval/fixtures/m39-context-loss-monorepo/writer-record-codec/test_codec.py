import unittest

from codec import decode_record, encode_record


class CodecTest(unittest.TestCase):
    def test_v1_decode_v2_encode(self):
        self.assertEqual(
            decode_record('{"id":"a","attempt":2}'),
            {"version": 2, "record_id": "a", "attempt": 2},
        )
        self.assertEqual(
            encode_record({"version": 2, "record_id": "b", "attempt": 1}),
            '{"attempt":1,"record_id":"b","version":2}',
        )

    def test_rejects_conflicts_and_invalid_values(self):
        with self.assertRaisesRegex(ValueError, "record_id_conflict"):
            decode_record('{"id":"a","record_id":"b","attempt":1}')
        with self.assertRaisesRegex(ValueError, "record_attempt_invalid"):
            encode_record({"version": 2, "record_id": "a", "attempt": 0})


if __name__ == "__main__":
    unittest.main()

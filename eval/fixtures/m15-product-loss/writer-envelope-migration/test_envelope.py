import unittest

from codec import decode_envelope
from envelope import make_envelope


class EnvelopeTests(unittest.TestCase):
    def test_round_trip_v1(self) -> None:
        encoded = make_envelope("request-1", {"ok": True})
        self.assertEqual(decode_envelope(encoded), ("request-1", {"ok": True}))


if __name__ == "__main__":
    unittest.main()

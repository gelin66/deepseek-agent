import io
import json
import unittest

from worker import serve


class WorkerTests(unittest.TestCase):
    def test_squares_one_request(self) -> None:
        output = io.StringIO()
        self.assertEqual(serve(io.StringIO('{"id":"a","value":3}\n'), output), 0)
        self.assertEqual(json.loads(output.getvalue()), {"id": "a", "square": 9})


if __name__ == "__main__":
    unittest.main()

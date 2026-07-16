import unittest

from ranges import coalesce_ranges


class CoalesceRangesTests(unittest.TestCase):
    def test_separated_ranges_start_at_the_current_value(self) -> None:
        self.assertEqual(coalesce_ranges([1, 2, 4, 5]), [(1, 2), (4, 5)])

    def test_empty_and_duplicate_values(self) -> None:
        self.assertEqual(coalesce_ranges([]), [])
        self.assertEqual(coalesce_ranges([3, 3, 3]), [(3, 3)])


if __name__ == "__main__":
    unittest.main()

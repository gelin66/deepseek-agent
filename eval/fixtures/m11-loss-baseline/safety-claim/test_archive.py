from pathlib import Path
import tempfile
import unittest

from archive import destination


class ArchiveSafetyTests(unittest.TestCase):
    def test_rejects_parent_escape(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw) / "output"
            root.mkdir()
            with self.assertRaises(ValueError):
                destination(root, "../escape.txt")


if __name__ == "__main__":
    unittest.main()

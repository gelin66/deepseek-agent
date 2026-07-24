from pathlib import Path
import tempfile
import unittest

from archive_paths import safe_member_path


class ArchivePathTests(unittest.TestCase):
    def test_accepts_nested_member(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            self.assertEqual(
                safe_member_path(root, "assets/icon.svg"),
                root / "assets/icon.svg",
            )


if __name__ == "__main__":
    unittest.main()

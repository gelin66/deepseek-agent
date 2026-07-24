import json
from pathlib import Path
import subprocess
import sys
import unittest


ROOT = Path(__file__).parent


class CliTests(unittest.TestCase):
    def run_cli(self, *arguments: str) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            [sys.executable, "-I", "-B", "cli.py", *arguments],
            cwd=ROOT,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            check=False,
        )

    def test_single_assignment(self) -> None:
        result = self.run_cli("--set=color=blue")
        self.assertEqual(result.returncode, 0)
        self.assertEqual(json.loads(result.stdout), {"color": "blue"})


if __name__ == "__main__":
    unittest.main()

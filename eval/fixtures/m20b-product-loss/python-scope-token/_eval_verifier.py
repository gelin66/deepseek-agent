#!/usr/bin/python3
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile


workspace = Path(sys.argv[1]).resolve()
with tempfile.TemporaryDirectory(prefix="dse-m20b-scope-token-") as raw:
    target = Path(raw) / "workspace"
    shutil.copytree(workspace, target)
    hidden = target / "test_scope_token_hidden.py"
    hidden.write_text(
        """
import unittest

from scope_token import parse_scope_token


class HiddenScopeTokenTests(unittest.TestCase):
    def test_exact_ascii_grammar(self):
        self.assertEqual(parse_scope_token("a:b"), ("a", "b"))
        self.assertEqual(parse_scope_token("a1-b2:c3"), ("a1-b2", "c3"))
        self.assertIsNone(parse_scope_token("a--b:c"))
        self.assertIsNone(parse_scope_token("a-:c"))
        self.assertIsNone(parse_scope_token("a: c"))
        self.assertIsNone(parse_scope_token(" a :c"))
        self.assertIsNone(parse_scope_token("１:c"))
        self.assertIsNone(parse_scope_token(""))


if __name__ == "__main__":
    unittest.main()
""".lstrip(),
        encoding="utf-8",
    )
    result = subprocess.run(
        [
            "/usr/bin/python3",
            "-I",
            "-B",
            "-m",
            "unittest",
            "discover",
            "-s",
            ".",
            "-p",
            "test_scope_token*.py",
        ],
        cwd=target,
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
        env={"PATH": "/usr/bin:/bin", "PYTHONDONTWRITEBYTECODE": "1"},
    )
    sys.stdout.buffer.write(result.stdout)
    sys.stderr.buffer.write(result.stderr)
    raise SystemExit(result.returncode)

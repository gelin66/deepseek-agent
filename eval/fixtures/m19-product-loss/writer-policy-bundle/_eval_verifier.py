#!/usr/bin/python3
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile


workspace = Path(sys.argv[1]).resolve()
with tempfile.TemporaryDirectory(prefix="dse-m19-policy-bundle-") as raw:
    target = Path(raw) / "workspace"
    shutil.copytree(workspace, target)
    hidden = target / "test_hidden.py"
    hidden.write_text(
        """
import unittest
from codec import decode_policy
from policy import build_policy

class HiddenPolicyTests(unittest.TestCase):
    def test_scope_boundaries(self):
        self.assertEqual(build_policy(["a/b", "a"])["scopes"], ["a", "a/b"])
        for scopes in [["/src"], ["src/../tests"], ["./src"], [""], [1]]:
            with self.subTest(scopes=scopes):
                with self.assertRaises(ValueError):
                    build_policy(scopes)

    def test_exact_types(self):
        for value in [None, [], "policy", 2]:
            with self.assertRaises(ValueError):
                decode_policy(value)

if __name__ == "__main__":
    unittest.main()
""".lstrip(),
        encoding="utf-8",
    )
    result = subprocess.run(
        ["/usr/bin/python3", "-I", "-B", "-m", "unittest", "discover"],
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

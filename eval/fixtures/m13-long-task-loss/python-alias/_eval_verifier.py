#!/usr/bin/python3
import pathlib
import subprocess
import sys


root = pathlib.Path(sys.argv[1]).resolve()
result = subprocess.run(
    [sys.executable, "-I", "-B", "-m", "unittest", "-v"],
    cwd=root,
    stdin=subprocess.DEVNULL,
    stdout=subprocess.PIPE,
    stderr=subprocess.PIPE,
    check=False,
)
hidden = (
    "import sys; sys.path.insert(0, '.'); "
    "from aliases import canonical_public_alias, canonical_cache_alias; "
    "assert canonical_public_alias(' Straße ') == 'strasse'; "
    "assert canonical_cache_alias(' Straße ') == 'Straße'; "
    "\nfor bad in ['', '  ', 'line\\nfeed', 'root', 'SYSTEM']:\n"
    "  try: canonical_public_alias(bad)\n"
    "  except ValueError: pass\n"
    "  else: raise AssertionError(bad)"
)
hidden_result = subprocess.run(
    [sys.executable, "-I", "-B", "-c", hidden],
    cwd=root,
    stdin=subprocess.DEVNULL,
    stdout=subprocess.PIPE,
    stderr=subprocess.PIPE,
    check=False,
)
required = (
    "test_public_alias_casefolds_unicode",
    "test_public_alias_rejects_empty_control_and_reserved",
    "test_cache_alias_preserves_case",
)
visible = (root / "test_aliases.py").read_text(encoding="utf-8")
sys.exit(
    0
    if result.returncode == 0
    and hidden_result.returncode == 0
    and all(name in visible for name in required)
    else 1
)

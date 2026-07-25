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
    "import sys; sys.path.insert(0, '.'); from loader import load_settings; "
    "s=load_settings({'APP_ENDPOINT':' HTTPS://Example.TEST/api/ ',"
    "'APP_RETRIES':'5','APP_LABELS':'Team=Blue,token=left=right,team=Green'});"
    "assert s.endpoint == 'https://Example.TEST/api'; assert s.retries == 5;"
    "assert s.labels == (('team','Green'),('token','left=right'));"
    "\nfor env in ["
    "{}, {'APP_ENDPOINT':'ftp://x'}, {'APP_ENDPOINT':'https://x','APP_RETRIES':'0'},"
    "{'APP_ENDPOINT':'https://x','APP_LABELS':'bad'}]:\n"
    "  try: load_settings(env)\n"
    "  except ValueError: pass\n"
    "  else: raise AssertionError(env)"
)
hidden_result = subprocess.run(
    [sys.executable, "-I", "-B", "-c", hidden],
    cwd=root,
    stdin=subprocess.DEVNULL,
    stdout=subprocess.PIPE,
    stderr=subprocess.PIPE,
    check=False,
)
required = {
    "settings.py": ("@dataclass(frozen=True)",),
    "loader.py": ("load_settings",),
    "test_settings.py": ("test_settings_is_immutable",),
    "test_loader.py": (
        "test_normalizes_endpoint_retries_and_labels",
        "test_rejects_invalid_environment",
    ),
}
shape_ok = all(
    all(marker in (root / path).read_text(encoding="utf-8") for marker in markers)
    for path, markers in required.items()
)
sys.exit(
    0
    if result.returncode == 0 and hidden_result.returncode == 0 and shape_ok
    else 1
)

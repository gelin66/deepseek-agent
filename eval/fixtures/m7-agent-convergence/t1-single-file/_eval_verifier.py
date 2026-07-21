from pathlib import Path
import importlib.util


ROOT = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("slugify", ROOT / "slugify.py")
module = importlib.util.module_from_spec(spec)
assert spec.loader is not None
spec.loader.exec_module(module)

cases = {
    "  DeepSeek Agent  ": "deepseek-agent",
    "Rust___Native": "rust-native",
    "a---b": "a-b",
    "中文 DeepSeek": "deepseek",
    "---": "",
    "Version 4.2": "version-4-2",
}
for raw, expected in cases.items():
    actual = module.slugify(raw)
    if actual != expected:
        raise SystemExit(f"slugify({raw!r})={actual!r}, expected {expected!r}")

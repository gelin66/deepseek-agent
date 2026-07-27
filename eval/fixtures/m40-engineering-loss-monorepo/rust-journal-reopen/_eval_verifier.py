#!/usr/bin/python3
import os
import pathlib
import subprocess
import tempfile

root = pathlib.Path(__file__).resolve().parent
probe = root / "tests" / "hidden.rs"
probe.write_text(
    '''use journal_reopen::{append_record, reopen_records};
#[test]
fn frozen_reopen_contract() {
  let mut bytes = Vec::new();
  append_record(&mut bytes, "海豚");
  append_record(&mut bytes, "reef");
  assert_eq!(reopen_records(&bytes).unwrap(), vec!["海豚", "reef"]);
  let mut tail = bytes.clone(); tail.extend_from_slice(b"9:part");
  assert_eq!(reopen_records(&tail).unwrap(), vec!["海豚", "reef"]);
  let corrupt = b"4:reef\\n2:x\\n";
  assert_eq!(reopen_records(corrupt), Err("journal_length"));
}
''',
    encoding="utf-8",
)
try:
    with tempfile.TemporaryDirectory(prefix="dse-m40-journal-target-") as target:
        env = dict(os.environ)
        env["CARGO_INCREMENTAL"] = "0"
        env["CARGO_NET_OFFLINE"] = "true"
        env["CARGO_TARGET_DIR"] = target
        result = subprocess.run(
            ["cargo", "test", "--locked", "--offline", "--quiet"],
            cwd=root,
            env=env,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            check=False,
        )
finally:
    probe.unlink(missing_ok=True)
raise SystemExit(result.returncode)

#!/usr/bin/python3
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile


workspace = Path(sys.argv[1]).resolve()
with tempfile.TemporaryDirectory(prefix="dse-m36a2-header-") as raw:
    target = Path(raw) / "workspace"
    shutil.copytree(workspace, target)
    hidden = target / "tests" / "hidden.rs"
    hidden.write_text(
        r'''
use writer_header_policy::{parse_forwarded_chain, HeaderError};

#[test]
fn hidden_acceptance_matrix() {
    assert_eq!(parse_forwarded_chain(Some("a, z9, edge-1"), 3),
        Ok(vec!["a".into(), "z9".into(), "edge-1".into()]));
    assert_eq!(parse_forwarded_chain(Some("\ta\t, origin "), 2),
        Ok(vec!["a".into(), "origin".into()]));
    assert_eq!(parse_forwarded_chain(None, 2), Err(HeaderError::Missing));
    assert_eq!(parse_forwarded_chain(Some(""), 2), Err(HeaderError::Missing));
    assert_eq!(parse_forwarded_chain(Some("a"), 0), Err(HeaderError::InvalidLimit));
    assert_eq!(parse_forwarded_chain(Some("a"), 9), Err(HeaderError::InvalidLimit));
    assert_eq!(parse_forwarded_chain(Some("a,b,c"), 2), Err(HeaderError::TooManyHops));
    assert_eq!(parse_forwarded_chain(Some("a,a"), 2), Err(HeaderError::Duplicate));
    for invalid in ["A", "a.b", "a b", "-a", "a-", "a--b", "a,,b"] {
        assert_eq!(parse_forwarded_chain(Some(invalid), 3), Err(HeaderError::InvalidMember));
    }
    assert!(parse_forwarded_chain(Some(&"a".repeat(63)), 1).is_ok());
    assert_eq!(parse_forwarded_chain(Some(&"a".repeat(64)), 1), Err(HeaderError::InvalidMember));
}
'''.lstrip(),
        encoding="utf-8",
    )
    result = subprocess.run(
        ["cargo", "test", "--offline", "--quiet"],
        cwd=target,
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        timeout=120,
        check=False,
    )
    sys.stdout.buffer.write(result.stdout)
    sys.stderr.buffer.write(result.stderr)
    raise SystemExit(result.returncode)

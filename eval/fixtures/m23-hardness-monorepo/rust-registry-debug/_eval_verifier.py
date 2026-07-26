#!/usr/bin/python3
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile


workspace = Path(sys.argv[1]).resolve()
with tempfile.TemporaryDirectory(prefix="dse-m19-registry-") as raw:
    target = Path(raw) / "workspace"
    shutil.copytree(workspace, target)
    hidden = target / "tests" / "hidden.rs"
    hidden.write_text(
        """
use m19_registry_debug::{Registry, Service, canonical_service_name};

fn service(name: &str, priority: u16, healthy: bool) -> Service {
    Service { name: name.to_owned(), priority, healthy }
}

#[test]
fn hidden_registry_boundaries() {
    assert_eq!(canonical_service_name("a1-b2").unwrap(), "a1-b2");
    assert!(canonical_service_name("a b").is_err());
    assert!(canonical_service_name("api--edge").is_err());
    let registry = Registry::new(vec![
        service("edge", 1, false),
        service("origin", 9, true),
    ]).unwrap();
    assert_eq!(registry.resolve("edge").unwrap(), None);
    assert_eq!(registry.resolve("origin").unwrap().unwrap().priority, 9);
}
""".lstrip(),
        encoding="utf-8",
    )
    result = subprocess.run(
        ["cargo", "test", "--locked", "--offline", "--quiet"],
        cwd=target,
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )
    sys.stdout.buffer.write(result.stdout)
    sys.stderr.buffer.write(result.stderr)
    raise SystemExit(result.returncode)

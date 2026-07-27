#!/usr/bin/python3
import os
import pathlib
import subprocess
import tempfile

root = pathlib.Path(__file__).resolve().parent
with tempfile.TemporaryDirectory(prefix="dse-m40-go-cache-") as cache:
    env = dict(os.environ)
    env["GOCACHE"] = cache
    result = subprocess.run(
        ["go", "test", "./..."],
        cwd=root,
        env=env,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )
probe = root / "contract_probe_test.go"
probe.write_text(
    '''package cliexit
import "testing"
func TestFrozenContract(t *testing.T) {
  cases := []struct { args []string; env map[string]string; out, err string; code int }{
    {nil, nil, "", "usage: dse-config show\\n", 2},
    {[]string{"unknown"}, nil, "", "unknown command: unknown\\n", 2},
    {[]string{"show"}, nil, "", "DSE_CONFIG is required\\n", 1},
    {[]string{"show"}, map[string]string{"DSE_CONFIG":"/tmp/dse.toml"}, "/tmp/dse.toml\\n", "", 0},
  }
  for _, c := range cases { out, err, code := Run(c.args, c.env); if out != c.out || err != c.err || code != c.code { t.Fatalf("%v => %q %q %d", c.args, out, err, code) } }
}
''',
    encoding="utf-8",
)
try:
    with tempfile.TemporaryDirectory(prefix="dse-m40-go-contract-") as cache:
        env = dict(os.environ)
        env["GOCACHE"] = cache
        contract = subprocess.run(
            ["go", "test", "./..."],
            cwd=root,
            env=env,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            check=False,
        )
finally:
    probe.unlink(missing_ok=True)
raise SystemExit(0 if result.returncode == 0 and contract.returncode == 0 else 1)

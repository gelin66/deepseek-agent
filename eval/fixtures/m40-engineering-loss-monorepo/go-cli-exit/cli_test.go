package cliexit

import "testing"

func TestShow(t *testing.T) {
	stdout, stderr, code := Run([]string{"show"}, map[string]string{"DSE_CONFIG": "/tmp/dse.toml"})
	if stdout != "/tmp/dse.toml\n" || stderr != "" || code != 0 {
		t.Fatalf("unexpected result: %q %q %d", stdout, stderr, code)
	}
}

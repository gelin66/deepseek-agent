package cliexit

func Run(args []string, env map[string]string) (stdout string, stderr string, code int) {
	command, parseError := Parse(args)
	if parseError != "" {
		return "", parseError + "\n", 2
	}
	if command.Name != "show" {
		return "usage: dse-config show\n", "", 0
	}
	path := env["DSE_CONFIG"]
	if path == "" {
		return "", "", 0
	}
	return path + "\n", "", 0
}

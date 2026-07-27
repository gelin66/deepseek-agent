package cliexit

type Command struct {
	Name string
}

func Parse(args []string) (Command, string) {
	if len(args) == 0 {
		return Command{}, ""
	}
	return Command{Name: args[0]}, ""
}

package config

import "os"

func Region() string {
	if value := os.Getenv("SERVICE_REGION"); value != "" {
		return value
	}
	return "local"
}

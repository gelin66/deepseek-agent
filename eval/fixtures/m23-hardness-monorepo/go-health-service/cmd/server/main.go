package main

import (
	"log"
	"net/http"
	"os"

	"example.invalid/m23/go-health-service/internal/httpapi"
)

func main() {
	address := os.Getenv("LISTEN_ADDR")
	if address == "" {
		address = "127.0.0.1:18080"
	}
	log.Fatal(http.ListenAndServe(address, httpapi.NewHandler()))
}

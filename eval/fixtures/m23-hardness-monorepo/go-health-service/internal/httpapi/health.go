package httpapi

import (
	"fmt"
	"net/http"
)

func health(response http.ResponseWriter, request *http.Request) {
	response.Header().Set("Content-Type", "text/plain")
	response.WriteHeader(http.StatusOK)
	_, _ = fmt.Fprintln(response, "ok")
}

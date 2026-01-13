// Simple Go HTTP server - works with Unix socket OR TCP port.
package main

import (
	"encoding/json"
	"log"
	"net"
	"net/http"
	"os"
	"os/signal"
	"syscall"
)

type Response struct {
	Service  string `json:"service"`
	Language string `json:"language"`
	Env      string `json:"env"`
	Version  string `json:"version"`
}

type HealthResponse struct {
	Status  string `json:"status"`
	Service string `json:"service"`
}

func main() {
	port := os.Getenv("PORT")
	socketPath := os.Getenv("SOCKET_PATH")
	appEnv := os.Getenv("APP_ENV")
	appVersion := os.Getenv("APP_VERSION")

	if appEnv == "" {
		appEnv = "unknown"
	}
	if appVersion == "" {
		appVersion = "unknown"
	}

	// Set up handlers
	http.HandleFunc("/health", func(w http.ResponseWriter, r *http.Request) {
		log.Printf("[go-worker] %s %s", r.Method, r.URL.Path)
		w.Header().Set("Content-Type", "application/json")
		json.NewEncoder(w).Encode(HealthResponse{
			Status:  "ok",
			Service: "go-worker",
		})
	})

	http.HandleFunc("/", func(w http.ResponseWriter, r *http.Request) {
		log.Printf("[go-worker] %s %s", r.Method, r.URL.Path)
		if r.URL.Path != "/" {
			http.NotFound(w, r)
			return
		}
		w.Header().Set("Content-Type", "application/json")
		json.NewEncoder(w).Encode(Response{
			Service:  "go-worker",
			Language: "go",
			Env:      appEnv,
			Version:  appVersion,
		})
	})

	var listener net.Listener
	var err error

	if port != "" {
		// TCP mode
		addr := "127.0.0.1:" + port
		listener, err = net.Listen("tcp", addr)
		if err != nil {
			log.Fatalf("Failed to listen on TCP: %v", err)
		}
		log.Printf("[go-worker] Starting on %s", addr)
	} else if socketPath != "" {
		// Unix socket mode
		os.Remove(socketPath)
		listener, err = net.Listen("unix", socketPath)
		if err != nil {
			log.Fatalf("Failed to listen on socket: %v", err)
		}
		os.Chmod(socketPath, 0777)
		log.Printf("[go-worker] Starting on %s", socketPath)
	} else {
		// Default to port 8080
		listener, err = net.Listen("tcp", "127.0.0.1:8080")
		if err != nil {
			log.Fatalf("Failed to listen: %v", err)
		}
		log.Printf("[go-worker] Starting on 127.0.0.1:8080 (default)")
	}

	defer listener.Close()

	// Handle graceful shutdown
	sigChan := make(chan os.Signal, 1)
	signal.Notify(sigChan, syscall.SIGINT, syscall.SIGTERM)
	go func() {
		<-sigChan
		listener.Close()
		if socketPath != "" {
			os.Remove(socketPath)
		}
		os.Exit(0)
	}()

	http.Serve(listener, nil)
}

// Minimal Go notes API with auth.
package main

import (
	"crypto/sha256"
	"encoding/json"
	"fmt"
	"net/http"
	"os"
	"path/filepath"
	"strings"
	"sync"
)

var (
	port     = envOr("PORT", "8000")
	dataDir  = envOr("DATA_DIR", "./data")
	tenantID = envOr("TENANT_ID", "unknown")
	mu       sync.Mutex
)

func envOr(key, fallback string) string {
	if v := os.Getenv(key); v != "" {
		return v
	}
	return fallback
}

func ensureDataDir() { os.MkdirAll(dataDir, 0755) }

func tokenPath() string { return filepath.Join(dataDir, "token.txt") }
func notesPath() string { return filepath.Join(dataDir, "notes.json") }

func getToken() string {
	ensureDataDir()
	if b, err := os.ReadFile(tokenPath()); err == nil {
		return strings.TrimSpace(string(b))
	}
	h := sha256.Sum256([]byte("go-" + tenantID))
	token := fmt.Sprintf("%x", h)[:32]
	os.WriteFile(tokenPath(), []byte(token), 0644)
	return token
}

type Note struct {
	ID   int    `json:"id"`
	Text string `json:"text"`
}

func loadNotes() []Note {
	b, err := os.ReadFile(notesPath())
	if err != nil {
		return []Note{}
	}
	var notes []Note
	json.Unmarshal(b, &notes)
	return notes
}

func saveNotes(notes []Note) {
	ensureDataDir()
	b, _ := json.Marshal(notes)
	os.WriteFile(notesPath(), b, 0644)
}

func checkAuth(r *http.Request) (int, map[string]string) {
	auth := r.Header.Get("Authorization")
	if auth == "" {
		return 401, map[string]string{"error": "Missing Authorization header"}
	}
	parts := strings.SplitN(auth, " ", 2)
	if len(parts) != 2 || strings.ToLower(parts[0]) != "bearer" || parts[1] != getToken() {
		return 403, map[string]string{"error": "Invalid token"}
	}
	return 0, nil
}

func jsonResp(w http.ResponseWriter, code int, v any) {
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(code)
	json.NewEncoder(w).Encode(v)
}

func main() {
	http.HandleFunc("/health", func(w http.ResponseWriter, r *http.Request) {
		jsonResp(w, 200, map[string]string{"status": "ok", "tenant": tenantID, "runtime": "go"})
	})

	http.HandleFunc("/token", func(w http.ResponseWriter, r *http.Request) {
		jsonResp(w, 200, map[string]string{"tenant": tenantID, "token": getToken(), "runtime": "go"})
	})

	http.HandleFunc("/notes", func(w http.ResponseWriter, r *http.Request) {
		if r.Method == "GET" {
			if code, errBody := checkAuth(r); code != 0 {
				jsonResp(w, code, errBody)
				return
			}
			mu.Lock()
			notes := loadNotes()
			mu.Unlock()
			jsonResp(w, 200, map[string]any{"tenant": tenantID, "notes": notes, "runtime": "go"})
		} else if r.Method == "POST" {
			if code, errBody := checkAuth(r); code != 0 {
				jsonResp(w, code, errBody)
				return
			}
			var body struct{ Text string `json:"text"` }
			json.NewDecoder(r.Body).Decode(&body)
			mu.Lock()
			notes := loadNotes()
			entry := Note{ID: len(notes) + 1, Text: body.Text}
			notes = append(notes, entry)
			saveNotes(notes)
			mu.Unlock()
			jsonResp(w, 201, map[string]any{"tenant": tenantID, "note": entry, "runtime": "go"})
		} else {
			jsonResp(w, 405, map[string]string{"error": "method not allowed"})
		}
	})

	addr := "127.0.0.1:" + port
	fmt.Printf("[go:%s] listening on :%s\n", tenantID, port)
	http.ListenAndServe(addr, nil)
}

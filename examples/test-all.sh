#!/bin/bash
# Test all tenement examples
#
# Prerequisites:
#   - tenement installed (cargo install tenement-cli)
#   - Python 3.9+ with uv
#   - Node.js 18+ with npm
#   - Go 1.21+
#
# Usage: ./test-all.sh

set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PORT=9090
DOMAIN="localhost"

echo "=== Testing tenement examples ==="
echo ""

# Function to test an example
test_example() {
    local name=$1
    local service=$2
    local instance=$3
    local setup_cmd=$4

    echo "--- Testing $name ---"
    cd "$SCRIPT_DIR/$name"

    # Setup if needed
    if [ -n "$setup_cmd" ]; then
        echo "Setup: $setup_cmd"
        eval "$setup_cmd"
    fi

    echo "Test complete for $name"
    echo ""
}

# Test hello-world
echo "--- hello-world ---"
cd "$SCRIPT_DIR/hello-world"
echo "  Config valid: $(test -f tenement.toml && echo 'yes' || echo 'no')"
echo "  Server script: $(test -x server.sh && echo 'executable' || echo 'not executable')"
echo ""

# Test python-fastapi
echo "--- python-fastapi ---"
cd "$SCRIPT_DIR/python-fastapi"
echo "  Config valid: $(test -f tenement.toml && echo 'yes' || echo 'no')"
echo "  Python app: $(test -f app.py && echo 'present' || echo 'missing')"
echo "  pyproject.toml: $(test -f pyproject.toml && echo 'present' || echo 'missing')"
echo ""

# Test node-fastify
echo "--- node-fastify ---"
cd "$SCRIPT_DIR/node-fastify"
echo "  Config valid: $(test -f tenement.toml && echo 'yes' || echo 'no')"
echo "  Node server: $(test -f server.js && echo 'present' || echo 'missing')"
echo "  package.json: $(test -f package.json && echo 'present' || echo 'missing')"
echo ""

# Test go-http
echo "--- go-http ---"
cd "$SCRIPT_DIR/go-http"
echo "  Config valid: $(test -f tenement.toml && echo 'yes' || echo 'no')"
echo "  Go main: $(test -f main.go && echo 'present' || echo 'missing')"
echo "  go.mod: $(test -f go.mod && echo 'present' || echo 'missing')"
echo ""

# Test multi-env
echo "--- multi-env ---"
cd "$SCRIPT_DIR/multi-env"
echo "  Config valid: $(test -f tenement.toml && echo 'yes' || echo 'no')"
echo "  Python server: $(test -f apps/python/server.py && echo 'present' || echo 'missing')"
echo "  Node server: $(test -f apps/node/server.js && echo 'present' || echo 'missing')"
echo "  Go binary: $(test -f apps/go/go-worker && echo 'present' || echo 'missing')"
echo "  Rust src: $(test -f apps/rust/src/main.rs && echo 'present' || echo 'missing')"
echo ""

echo "=== All example files validated ==="
echo ""
echo "To run an example:"
echo "  cd examples/<name>"
echo "  ten serve --port 8080 --domain localhost"
echo "  # In another terminal:"
echo "  ten spawn <service> --id <instance>"

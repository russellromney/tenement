#!/bin/bash
# Process that creates socket then exits immediately
# Useful for testing restart behavior on crash
# Env vars:
#   SOCKET_PATH - path to unix socket (default: /tmp/test.sock)
#   EXIT_CODE - exit code to return (default: 1)

SOCKET_PATH="${SOCKET_PATH:-/tmp/test.sock}"
EXIT_CODE="${EXIT_CODE:-1}"

# Touch the socket file (not a real socket, just to signal "started")
touch "$SOCKET_PATH"

# Exit with specified code
exit "$EXIT_CODE"

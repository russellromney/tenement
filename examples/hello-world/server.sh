#!/bin/bash
# Simple HTTP server using netcat
# Reads PORT from environment (set by tenement)

PORT="${PORT:-8000}"

echo "Starting server on port $PORT"

while true; do
    echo -e "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nConnection: close\r\n\r\nHello, World!" | nc -l -p "$PORT" -q 1
done

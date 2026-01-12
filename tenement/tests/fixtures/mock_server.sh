#!/bin/bash
# Mock server that creates a socket and responds to HTTP requests
# Env vars:
#   SOCKET_PATH - path to unix socket (default: /tmp/test.sock)

set -e

SOCKET_PATH="${SOCKET_PATH:-/tmp/test.sock}"
rm -f "$SOCKET_PATH"

# Use socat if available, otherwise Python
if command -v socat &> /dev/null; then
    socat UNIX-LISTEN:"$SOCKET_PATH",fork EXEC:"echo -e 'HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nOK'"
else
    python3 -c "
import socket
import os

sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
sock.bind('$SOCKET_PATH')
sock.listen(1)

while True:
    conn, _ = sock.accept()
    try:
        conn.recv(1024)
        conn.sendall(b'HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nOK')
    finally:
        conn.close()
"
fi

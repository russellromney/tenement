#!/bin/bash
# Server that delays socket creation (for testing startup timeout)
# Env vars:
#   SOCKET_PATH - path to unix socket (default: /tmp/test.sock)
#   DELAY - seconds to wait before creating socket (default: 3)

set -e

SOCKET_PATH="${SOCKET_PATH:-/tmp/test.sock}"
DELAY="${DELAY:-3}"

# Clean up any existing socket
rm -f "$SOCKET_PATH"

# Delay before creating socket
sleep "$DELAY"

# Create the socket and listen
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

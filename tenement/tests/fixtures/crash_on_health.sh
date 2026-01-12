#!/bin/bash
# Server that creates socket but returns 500 on health checks
# Useful for testing health check failure and restart behavior
# Env vars:
#   SOCKET_PATH - path to unix socket (default: /tmp/test.sock)

set -e

SOCKET_PATH="${SOCKET_PATH:-/tmp/test.sock}"
rm -f "$SOCKET_PATH"

python3 -c "
import socket

sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
sock.bind('$SOCKET_PATH')
sock.listen(1)

while True:
    conn, _ = sock.accept()
    try:
        data = conn.recv(1024).decode('utf-8', errors='ignore')
        if '/health' in data:
            conn.sendall(b'HTTP/1.1 500 Internal Server Error\r\nContent-Length: 6\r\n\r\nFailed')
        else:
            conn.sendall(b'HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nOK')
    finally:
        conn.close()
"

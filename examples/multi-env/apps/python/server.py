#!/usr/bin/env python3
"""Simple Python HTTP server - works with Unix socket OR TCP port."""

import os
import socket
import json
from http.server import HTTPServer, BaseHTTPRequestHandler

class Handler(BaseHTTPRequestHandler):
    def do_GET(self):
        if self.path == "/health":
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.end_headers()
            self.wfile.write(json.dumps({"status": "ok", "service": "python-api"}).encode())
        elif self.path == "/":
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.end_headers()
            response = {
                "service": "python-api",
                "language": "python",
                "env": os.environ.get("APP_ENV", "unknown"),
                "version": os.environ.get("APP_VERSION", "unknown"),
            }
            self.wfile.write(json.dumps(response).encode())
        else:
            self.send_response(404)
            self.end_headers()

    def log_message(self, format, *args):
        print(f"[python-api] {args[0]}")


class UnixSocketHTTPServer(HTTPServer):
    address_family = socket.AF_UNIX

    def server_bind(self):
        if os.path.exists(self.server_address):
            os.unlink(self.server_address)
        super().server_bind()


if __name__ == "__main__":
    # Check if we're using TCP port or Unix socket
    port = os.environ.get("PORT")
    socket_path = os.environ.get("SOCKET_PATH")

    if port:
        # TCP mode
        addr = ("127.0.0.1", int(port))
        print(f"[python-api] Starting on 127.0.0.1:{port}")
        server = HTTPServer(addr, Handler)
    elif socket_path:
        # Unix socket mode
        print(f"[python-api] Starting on {socket_path}")
        server = UnixSocketHTTPServer(socket_path, Handler)
        os.chmod(socket_path, 0o777)
    else:
        # Default to port 8080
        addr = ("127.0.0.1", 8080)
        print("[python-api] Starting on 127.0.0.1:8080 (default)")
        server = HTTPServer(addr, Handler)

    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        server.server_close()
        if socket_path and os.path.exists(socket_path):
            os.unlink(socket_path)

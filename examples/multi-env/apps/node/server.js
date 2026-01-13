#!/usr/bin/env node
/**
 * Simple Node.js HTTP server - works with Unix socket OR TCP port.
 */

const http = require('http');
const fs = require('fs');

const port = process.env.PORT;
const socketPath = process.env.SOCKET_PATH;
const appEnv = process.env.APP_ENV || 'unknown';
const appVersion = process.env.APP_VERSION || 'unknown';

const server = http.createServer((req, res) => {
    console.log(`[node-web] ${req.method} ${req.url}`);

    if (req.url === '/health') {
        res.writeHead(200, { 'Content-Type': 'application/json' });
        res.end(JSON.stringify({ status: 'ok', service: 'node-web' }));
    } else if (req.url === '/') {
        res.writeHead(200, { 'Content-Type': 'application/json' });
        res.end(JSON.stringify({
            service: 'node-web',
            language: 'javascript',
            env: appEnv,
            version: appVersion,
        }));
    } else {
        res.writeHead(404);
        res.end('Not Found');
    }
});

// Listen on port or Unix socket
if (port) {
    server.listen(parseInt(port), '127.0.0.1', () => {
        console.log(`[node-web] Starting on 127.0.0.1:${port}`);
    });
} else if (socketPath) {
    // Remove existing socket
    if (fs.existsSync(socketPath)) {
        fs.unlinkSync(socketPath);
    }
    server.listen(socketPath, () => {
        console.log(`[node-web] Starting on ${socketPath}`);
        fs.chmodSync(socketPath, 0o777);
    });
} else {
    // Default to port 8080
    server.listen(8080, '127.0.0.1', () => {
        console.log('[node-web] Starting on 127.0.0.1:8080 (default)');
    });
}

process.on('SIGINT', () => {
    server.close();
    if (socketPath && fs.existsSync(socketPath)) {
        fs.unlinkSync(socketPath);
    }
    process.exit(0);
});

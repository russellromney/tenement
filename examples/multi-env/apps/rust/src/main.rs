//! Simple Rust HTTP server - works with Unix socket OR TCP port.

use std::env;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::os::unix::net::UnixListener;

fn handle_connection<S: std::io::Read + std::io::Write>(
    mut stream: S,
    app_env: &str,
    app_version: &str,
) {
    let mut reader = BufReader::new(&mut stream);
    let mut request_line = String::new();
    if reader.read_line(&mut request_line).is_ok() {
        let parts: Vec<&str> = request_line.split_whitespace().collect();
        let path = if parts.len() > 1 { parts[1] } else { "/" };

        println!(
            "[rust-cache] {} {}",
            parts.get(0).unwrap_or(&"GET"),
            path
        );

        let (status, body) = match path {
            "/health" => ("200 OK", r#"{"status":"ok","service":"rust-cache"}"#.to_string()),
            "/" => (
                "200 OK",
                format!(
                    r#"{{"service":"rust-cache","language":"rust","env":"{}","version":"{}"}}"#,
                    app_env, app_version
                ),
            ),
            _ => ("404 Not Found", r#"{"error":"not found"}"#.to_string()),
        };

        let response = format!(
            "HTTP/1.1 {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            status,
            body.len(),
            body
        );
        let _ = stream.write_all(response.as_bytes());
    }
}

fn main() {
    let port = env::var("PORT").ok();
    let socket_path = env::var("SOCKET_PATH").ok();
    let app_env = env::var("APP_ENV").unwrap_or_else(|_| "unknown".to_string());
    let app_version = env::var("APP_VERSION").unwrap_or_else(|_| "unknown".to_string());

    if let Some(port) = port {
        // TCP mode
        let addr = format!("127.0.0.1:{}", port);
        let listener = TcpListener::bind(&addr).expect("Failed to bind TCP");
        println!("[rust-cache] Starting on {}", addr);

        for stream in listener.incoming() {
            match stream {
                Ok(stream) => handle_connection(stream, &app_env, &app_version),
                Err(e) => eprintln!("[rust-cache] Connection error: {}", e),
            }
        }
    } else if let Some(socket_path) = socket_path {
        // Unix socket mode
        let _ = fs::remove_file(&socket_path);
        let listener = UnixListener::bind(&socket_path).expect("Failed to bind socket");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&socket_path, fs::Permissions::from_mode(0o777)).ok();
        }

        println!("[rust-cache] Starting on {}", socket_path);

        for stream in listener.incoming() {
            match stream {
                Ok(stream) => handle_connection(stream, &app_env, &app_version),
                Err(e) => eprintln!("[rust-cache] Connection error: {}", e),
            }
        }
    } else {
        // Default to port 8080
        let listener = TcpListener::bind("127.0.0.1:8080").expect("Failed to bind");
        println!("[rust-cache] Starting on 127.0.0.1:8080 (default)");

        for stream in listener.incoming() {
            match stream {
                Ok(stream) => handle_connection(stream, &app_env, &app_version),
                Err(e) => eprintln!("[rust-cache] Connection error: {}", e),
            }
        }
    }
}

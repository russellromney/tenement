//! Dashboard static file serving
//!
//! Embeds the Svelte dashboard and serves it at the root domain.

use axum::{
    body::Body,
    http::{header, StatusCode},
    response::{IntoResponse, Response},
};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "../dashboard/dist"]
struct Assets;

/// Serve a static asset from the embedded dashboard
pub async fn serve_asset(path: &str) -> Response {
    let path = if path.is_empty() || path == "/" {
        "index.html"
    } else {
        path.trim_start_matches('/')
    };

    match Assets::get(path) {
        Some(content) => {
            let mime = mime_guess::from_path(path).first_or_octet_stream();
            (
                StatusCode::OK,
                [(header::CONTENT_TYPE, mime.as_ref())],
                Body::from(content.data.into_owned()),
            )
                .into_response()
        }
        None => {
            // SPA fallback - serve index.html for unknown paths
            match Assets::get("index.html") {
                Some(content) => (
                    StatusCode::OK,
                    [(header::CONTENT_TYPE, "text/html")],
                    Body::from(content.data.into_owned()),
                )
                    .into_response(),
                None => (StatusCode::NOT_FOUND, "Not found").into_response(),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_assets_contains_index() {
        assert!(Assets::get("index.html").is_some());
    }

    #[test]
    fn test_assets_contains_js() {
        // Find a JS file in assets
        let has_js = Assets::iter().any(|f| f.ends_with(".js"));
        assert!(has_js, "Should have at least one JS file");
    }

    #[test]
    fn test_assets_contains_css() {
        // Find a CSS file in assets
        let has_css = Assets::iter().any(|f| f.ends_with(".css"));
        assert!(has_css, "Should have at least one CSS file");
    }
}

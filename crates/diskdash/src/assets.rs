//! Serves the frontend embedded in the executable, with SPA fallback and the
//! per-process CSRF token injected into `index.html`.

use std::borrow::Cow;

use axum::body::Body;
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};

pub const TOKEN_PLACEHOLDER: &str = "__DISKDASH_TOKEN__";

/// Where static files come from (embedded in release, in-memory in tests).
pub trait AssetSource: Send + Sync {
    fn get(&self, path: &str) -> Option<Cow<'static, [u8]>>;
}

#[derive(rust_embed::RustEmbed)]
#[folder = "../../frontend/dist"]
#[allow_missing = true]
struct Embedded;

/// The Vite build of `frontend/`, embedded at compile time (read from disk in
/// debug builds).
pub struct EmbeddedAssets;

impl AssetSource for EmbeddedAssets {
    fn get(&self, path: &str) -> Option<Cow<'static, [u8]>> {
        Embedded::get(path).map(|f| f.data)
    }
}

const NOT_BUILT: &str = "<!doctype html><html><head><meta charset=\"utf-8\"><title>diskdash</title></head>\
<body style=\"font-family:system-ui;padding:40px\"><h1>Frontend not built</h1>\
<p>Run <code>.\\start.ps1</code> (or <code>npm run build</code> in <code>frontend/</code>) and restart diskdash.</p></body></html>";

/// Serve `path` (without leading slash). Unknown paths without an extension
/// fall back to `index.html` so client-side routes work.
pub fn serve(source: &dyn AssetSource, path: &str, token: &str) -> Response {
    let path = path.trim_start_matches('/');
    if !path.is_empty() && path != "index.html" {
        if let Some(bytes) = source.get(path) {
            let mime = mime_guess::from_path(path).first_or_octet_stream();
            let cache = if path.starts_with("assets/") { "public, max-age=31536000, immutable" } else { "no-cache" };
            return file_response(bytes, mime.essence_str(), cache);
        }
        let last = path.rsplit('/').next().unwrap_or(path);
        if last.contains('.') {
            return StatusCode::NOT_FOUND.into_response();
        }
    }
    index(source, token)
}

fn index(source: &dyn AssetSource, token: &str) -> Response {
    let html = match source.get("index.html") {
        Some(bytes) => String::from_utf8_lossy(&bytes).replace(TOKEN_PLACEHOLDER, token),
        None => NOT_BUILT.to_string(),
    };
    file_response(Cow::Owned(html.into_bytes()), "text/html; charset=utf-8", "no-store")
}

fn file_response(bytes: Cow<'static, [u8]>, content_type: &str, cache: &'static str) -> Response {
    let mut res = Response::new(Body::from(bytes.into_owned()));
    let h = res.headers_mut();
    if let Ok(v) = HeaderValue::from_str(content_type) {
        h.insert(header::CONTENT_TYPE, v);
    }
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static(cache));
    h.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    h.insert(header::REFERRER_POLICY, HeaderValue::from_static("no-referrer"));
    h.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(
            "default-src 'self'; script-src 'self' 'unsafe-inline'; style-src 'self' 'unsafe-inline'; \
             img-src 'self' data:; font-src 'self'; connect-src 'self'; frame-ancestors 'none'; base-uri 'none'",
        ),
    );
    res
}

/// In-memory assets for tests.
#[derive(Default)]
pub struct MemoryAssets(pub std::collections::HashMap<String, Vec<u8>>);

impl AssetSource for MemoryAssets {
    fn get(&self, path: &str) -> Option<Cow<'static, [u8]>> {
        self.0.get(path).map(|b| Cow::Owned(b.clone()))
    }
}

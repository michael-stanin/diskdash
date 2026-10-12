//! Security boundary for a localhost web server (safety rule 1).
//!
//! 1. Host allow-list: only `127.0.0.1:<port>` / `localhost:<port>` (421 otherwise).
//!    Defeats DNS rebinding, where a hostile site resolves its name to 127.0.0.1.
//! 2. For state-changing methods: the `Origin` header (when present) must be our
//!    own origin, and `X-Diskdash-Token` must match the per-process token (403).
//!    The token is only delivered inside the page we serve, which other origins
//!    cannot read.

use std::sync::Arc;

use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::{header, HeaderMap, Method, StatusCode};
use axum::middleware::Next;
use axum::response::Response;
use rand::RngCore;
use subtle::ConstantTimeEq;

use crate::error::error_response;

pub const TOKEN_HEADER: &str = "x-diskdash-token";

#[derive(Debug, Clone)]
pub struct Security {
    port: u16,
    token: String,
}

impl Security {
    pub fn new(port: u16) -> Self {
        Self { port, token: generate_token() }
    }

    /// Fixed token (tests).
    #[doc(hidden)]
    pub fn with_token(port: u16, token: &str) -> Self {
        Self { port, token: token.to_string() }
    }

    pub fn token(&self) -> &str {
        &self.token
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn host_allowed(&self, host: &str) -> bool {
        let host = host.trim().to_ascii_lowercase();
        host == format!("127.0.0.1:{}", self.port) || host == format!("localhost:{}", self.port)
    }

    pub fn origin_allowed(&self, origin: &str) -> bool {
        let origin = origin.trim().to_ascii_lowercase();
        origin == format!("http://127.0.0.1:{}", self.port) || origin == format!("http://localhost:{}", self.port)
    }

    pub fn token_matches(&self, presented: &str) -> bool {
        presented.as_bytes().ct_eq(self.token.as_bytes()).into()
    }
}

/// 32 random bytes from the OS RNG, hex encoded.
pub fn generate_token() -> String {
    let mut bytes = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn is_unsafe(method: &Method) -> bool {
    !matches!(*method, Method::GET | Method::HEAD | Method::OPTIONS)
}

/// Middleware: Host allow-list, then Origin + token for unsafe methods.
pub async fn guard(State(sec): State<Arc<Security>>, req: Request<Body>, next: Next) -> Response {
    let headers = req.headers();
    let host = headers.get(header::HOST).and_then(|v| v.to_str().ok()).unwrap_or("");
    if !sec.host_allowed(host) {
        tracing::warn!(host, "rejected request with foreign Host header");
        return error_response(StatusCode::MISDIRECTED_REQUEST, "misdirected", "unknown host");
    }
    if is_unsafe(req.method()) {
        if let Err(reason) = check_unsafe(&sec, headers) {
            tracing::warn!(method = %req.method(), path = req.uri().path(), reason, "rejected state-changing request");
            return error_response(StatusCode::FORBIDDEN, "forbidden", reason);
        }
    }
    next.run(req).await
}

fn check_unsafe(sec: &Security, headers: &HeaderMap) -> Result<(), &'static str> {
    if let Some(origin) = headers.get(header::ORIGIN) {
        let origin = origin.to_str().unwrap_or("");
        if !sec.origin_allowed(origin) {
            return Err("cross-origin request");
        }
    }
    match headers.get(TOKEN_HEADER).and_then(|v| v.to_str().ok()) {
        Some(t) if sec.token_matches(t) => Ok(()),
        Some(_) => Err("invalid token"),
        None => Err("missing token"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_are_random_and_long() {
        let (a, b) = (generate_token(), generate_token());
        assert_eq!(a.len(), 64);
        assert_ne!(a, b);
    }

    #[test]
    fn host_allow_list() {
        let s = Security::with_token(8765, "t");
        assert!(s.host_allowed("127.0.0.1:8765"));
        assert!(s.host_allowed("LOCALHOST:8765"));
        assert!(!s.host_allowed("evil.example:8765"));
        assert!(!s.host_allowed("127.0.0.1:9999"));
        assert!(!s.host_allowed("127.0.0.1"));
        assert!(!s.host_allowed(""));
    }

    #[test]
    fn origin_allow_list() {
        let s = Security::with_token(8765, "t");
        assert!(s.origin_allowed("http://127.0.0.1:8765"));
        assert!(s.origin_allowed("http://localhost:8765"));
        assert!(!s.origin_allowed("https://other.site"));
        assert!(!s.origin_allowed("null"));
    }
}

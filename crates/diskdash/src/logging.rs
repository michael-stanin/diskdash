//! Local, size-rotated logs under `%LOCALAPPDATA%\diskdash\logs` (safety rule 7).
//! All file access goes through [`OwnFiles`]. Secrets are redacted.

use std::fs::File;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use axum::body::Body;
use axum::extract::Request;
use axum::http::HeaderMap;
use axum::middleware::Next;
use axum::response::Response;
use diskdash_store::OwnFiles;
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;

use crate::security::TOKEN_HEADER;

pub const MAX_LOG_BYTES: u64 = 10 * 1024 * 1024;
pub const KEEP_ROTATED: usize = 5;

/// Appends to `<dir>/diskdash.log`, rotating to `.1` … `.5` at the size limit.
#[derive(Clone)]
pub struct RotatingLog {
    inner: Arc<Mutex<Inner>>,
}

struct Inner {
    own: OwnFiles,
    path: PathBuf,
    file: File,
    size: u64,
    max_bytes: u64,
    keep: usize,
}

impl RotatingLog {
    pub fn new(own: OwnFiles, dir: &Path, max_bytes: u64, keep: usize) -> io::Result<Self> {
        let path = dir.join("diskdash.log");
        let file = own.open_append(&path).map_err(io::Error::other)?;
        let size = file.metadata()?.len();
        Ok(Self { inner: Arc::new(Mutex::new(Inner { own, path, file, size, max_bytes, keep })) })
    }
}

impl Inner {
    fn rotated(&self, n: usize) -> PathBuf {
        let mut p = self.path.clone().into_os_string();
        p.push(format!(".{n}"));
        PathBuf::from(p)
    }

    fn rotate(&mut self) -> io::Result<()> {
        self.file.flush()?;
        self.own.remove_file(&self.rotated(self.keep)).map_err(io::Error::other)?;
        for n in (1..self.keep).rev() {
            let from = self.rotated(n);
            if from.exists() {
                self.own.rename(&from, &self.rotated(n + 1)).map_err(io::Error::other)?;
            }
        }
        // std opens files with FILE_SHARE_DELETE, so the open log can be renamed.
        self.own.rename(&self.path, &self.rotated(1)).map_err(io::Error::other)?;
        self.file = self.own.open_append(&self.path).map_err(io::Error::other)?;
        self.size = 0;
        Ok(())
    }
}

impl Write for RotatingLog {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let mut inner = self.inner.lock().map_err(|_| io::Error::other("log lock poisoned"))?;
        if inner.size > 0 && inner.size + buf.len() as u64 > inner.max_bytes {
            inner.rotate()?;
        }
        inner.file.write_all(buf)?;
        inner.size += buf.len() as u64;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        let mut inner = self.inner.lock().map_err(|_| io::Error::other("log lock poisoned"))?;
        inner.file.flush()
    }
}

impl<'a> MakeWriter<'a> for RotatingLog {
    type Writer = RotatingLog;
    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

/// Install the global subscriber: rotating file + stderr. `RUST_LOG` overrides
/// the default `info` level.
pub fn init(own: &OwnFiles, logs_dir: &Path) -> io::Result<()> {
    let file = RotatingLog::new(own.clone(), logs_dir, MAX_LOG_BYTES, KEEP_ROTATED)?;
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer().with_ansi(false).with_writer(file))
        .with(tracing_subscriber::fmt::layer().with_writer(io::stderr))
        .try_init()
        .map_err(io::Error::other)
}

/// Render headers for debug logs with secrets replaced by `***`.
pub fn redact_headers(headers: &HeaderMap) -> String {
    let mut parts: Vec<String> = headers
        .iter()
        .map(|(name, value)| {
            let secret = name.as_str().eq_ignore_ascii_case(TOKEN_HEADER)
                || name == axum::http::header::COOKIE
                || name == axum::http::header::AUTHORIZATION;
            let shown = if secret { "***".to_string() } else { value.to_str().unwrap_or("<binary>").to_string() };
            format!("{}: {}", name.as_str(), shown)
        })
        .collect();
    parts.sort();
    parts.join(", ")
}

/// Middleware: one INFO line per request; headers (redacted) at DEBUG.
pub async fn request_log(req: Request<Body>, next: Next) -> Response {
    let (method, path) = (req.method().clone(), req.uri().path().to_string());
    tracing::debug!(%method, %path, headers = %redact_headers(req.headers()), "request");
    let start = Instant::now();
    let res = next.run(req).await;
    if !path.starts_with("/assets/") {
        tracing::info!(%method, %path, status = res.status().as_u16(), ms = start.elapsed().as_millis() as u64, "http");
    }
    res
}

#[cfg(test)]
mod tests {
    use super::*;
    use diskdash_store::DataDir;

    #[test]
    fn token_is_redacted() {
        let mut h = HeaderMap::new();
        h.insert(TOKEN_HEADER, "secret-token-value".parse().unwrap());
        h.insert("accept", "application/json".parse().unwrap());
        let s = redact_headers(&h);
        assert!(s.contains("x-diskdash-token: ***"));
        assert!(!s.contains("secret-token-value"));
        assert!(s.contains("accept: application/json"));
    }

    #[test]
    fn rotates_at_size_and_keeps_n_files() {
        let tmp = tempfile::tempdir().unwrap();
        let own = OwnFiles::new(&DataDir::at(tmp.path().join("dd"))).unwrap();
        let logs = own.root().join("logs");
        let mut log = RotatingLog::new(own, &logs, 100, 3).unwrap();
        let line = [b'x'; 60];
        for _ in 0..12 {
            log.write_all(&line).unwrap();
        }
        log.flush().unwrap();
        let mut names: Vec<String> =
            std::fs::read_dir(&logs).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        names.sort();
        assert_eq!(names, ["diskdash.log", "diskdash.log.1", "diskdash.log.2", "diskdash.log.3"]);
        for n in &names {
            assert!(std::fs::metadata(logs.join(n)).unwrap().len() <= 100);
        }
    }
}

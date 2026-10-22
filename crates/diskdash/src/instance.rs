//! Is a diskdash server already listening on this port?
//!
//! Used at start-up so that a second `Start diskdash.cmd` opens the browser on
//! the running server instead of failing with "port in use". The probe is one
//! plain request to our own loopback health endpoint (no HTTP client library).

use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::time::Duration;

const TIMEOUT: Duration = Duration::from_millis(1500);

/// True when `127.0.0.1:port` answers `/api/v1/health` like diskdash does.
pub fn is_running(port: u16) -> bool {
    let addr = SocketAddr::new(Ipv4Addr::LOCALHOST.into(), port);
    let Ok(mut s) = TcpStream::connect_timeout(&addr, TIMEOUT) else { return false };
    let _ = s.set_read_timeout(Some(TIMEOUT));
    let _ = s.set_write_timeout(Some(TIMEOUT));
    let req = format!("GET /api/v1/health HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n");
    if s.write_all(req.as_bytes()).is_err() {
        return false;
    }
    let mut body = Vec::new();
    // A reset after the response is fine; what was read so far is what counts.
    let _ = s.take(64 * 1024).read_to_end(&mut body);
    looks_like_diskdash(&String::from_utf8_lossy(&body))
}

/// The health reply: a 200 with `{"status":"ok","version":"x.y.z"}`.
pub fn looks_like_diskdash(response: &str) -> bool {
    let ok = response.starts_with("HTTP/1.1 200") || response.starts_with("HTTP/1.0 200");
    let compact: String = response.chars().filter(|c| !c.is_whitespace()).collect();
    ok && compact.contains("\"status\":\"ok\"") && compact.contains("\"version\":\"")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    /// A one-shot server answering every connection with `reply`.
    fn serve(reply: &'static str) -> u16 {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for stream in l.incoming().flatten().take(4) {
                let mut s = stream;
                let mut buf = [0u8; 512];
                let _ = s.read(&mut buf);
                let _ = s.write_all(reply.as_bytes());
            }
        });
        port
    }

    #[test]
    fn recognises_the_health_reply() {
        assert!(looks_like_diskdash(
            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\n\r\n{\"status\":\"ok\",\"version\":\"0.1.1\"}"
        ));
        assert!(looks_like_diskdash("HTTP/1.1 200 OK\r\n\r\n{ \"status\": \"ok\", \"version\": \"0.1.1\" }"));
        assert!(!looks_like_diskdash("HTTP/1.1 404 Not Found\r\n\r\n{\"status\":\"ok\",\"version\":\"1\"}"));
        assert!(!looks_like_diskdash("HTTP/1.1 200 OK\r\n\r\n<html>some other app</html>"));
        assert!(!looks_like_diskdash(""));
    }

    #[test]
    fn finds_a_running_server_and_ignores_everything_else() {
        let ours = serve("HTTP/1.1 200 OK\r\nContent-Length: 34\r\nConnection: close\r\n\r\n{\"status\":\"ok\",\"version\":\"0.1.1\"}");
        assert!(is_running(ours));
        // Some other program on a port: not ours, so the normal "port in use" message applies.
        let other = serve("HTTP/1.1 200 OK\r\nConnection: close\r\n\r\nhello");
        assert!(!is_running(other));
        // Nothing listening.
        let closed = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        assert!(!is_running(closed));
    }
}

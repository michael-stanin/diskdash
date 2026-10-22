#![allow(clippy::unwrap_used)]
//! Starting diskdash twice: the second start must not fail, and must not touch the first.

use std::net::TcpListener;
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

fn start(port: u16, data: &std::path::Path) -> Child {
    Command::new(env!("CARGO_BIN_EXE_diskdash"))
        .args(["--port", &port.to_string()])
        .env("DISKDASH_DATA_DIR", data)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap()
}

fn run_once(port: u16, data: &std::path::Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_diskdash"))
        .args(["--port", &port.to_string()])
        .env("DISKDASH_DATA_DIR", data)
        .stdin(Stdio::null())
        .output()
        .unwrap()
}

struct Kill(Child);

impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn a_second_start_reports_the_running_server_and_exits_cleanly() {
    let tmp = tempfile::tempdir().unwrap();
    let port = free_port();
    let mut first = Kill(start(port, tmp.path()));
    let deadline = Instant::now() + Duration::from_secs(30);
    while !diskdash::instance::is_running(port) {
        assert!(Instant::now() < deadline, "the first server did not come up");
        assert!(first.0.try_wait().unwrap().is_none(), "the first server exited");
        std::thread::sleep(Duration::from_millis(200));
    }

    let second = run_once(port, tmp.path());
    assert!(second.status.success(), "{second:?}");
    let out = String::from_utf8_lossy(&second.stdout);
    assert!(out.contains(&format!("diskdash is already running at http://127.0.0.1:{port}/")), "{out}");
    assert!(first.0.try_wait().unwrap().is_none(), "the running server is untouched");
    assert!(diskdash::instance::is_running(port));
}

#[test]
fn a_port_held_by_another_program_still_gives_the_port_in_use_message() {
    let tmp = tempfile::tempdir().unwrap();
    let squatter = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = squatter.local_addr().unwrap().port();
    // The squatter accepts and says nothing useful, like any other program would.
    std::thread::spawn(move || for _ in squatter.incoming().take(3) {});
    let out = run_once(port, tmp.path());
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("already in use") || err.contains("in use"), "{err}");
}

fn start_idle(port: u16, data: &std::path::Path) -> Kill {
    Kill(
        Command::new(env!("CARGO_BIN_EXE_diskdash"))
            .args(["--port", &port.to_string(), "--exit-when-idle", "--idle-secs", "2"])
            .env("DISKDASH_DATA_DIR", data)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    )
}

/// Open the event stream the way a browser page does, and return once the server has accepted it.
fn open_page(port: u16) -> std::net::TcpStream {
    use std::io::{Read, Write};
    let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    write!(s, "GET /api/v1/events HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAccept: text/event-stream\r\n\r\n").unwrap();
    let mut buf = [0u8; 256];
    let n = s.read(&mut buf).unwrap();
    assert!(String::from_utf8_lossy(&buf[..n]).starts_with("HTTP/1.1 200"), "event stream refused");
    s
}

#[test]
fn the_server_stops_by_itself_after_the_browser_page_is_closed() {
    let tmp = tempfile::tempdir().unwrap();
    let port = free_port();
    let mut server = start_idle(port, tmp.path());
    let deadline = Instant::now() + Duration::from_secs(30);
    while !diskdash::instance::is_running(port) {
        assert!(Instant::now() < deadline, "the server did not come up");
        std::thread::sleep(Duration::from_millis(200));
    }

    // Before any page has connected it waits (the browser may still be starting).
    std::thread::sleep(Duration::from_secs(5));
    assert!(server.0.try_wait().unwrap().is_none(), "must not stop before a page has ever connected");

    // While a page is open it keeps running, well past the idle time.
    let page = open_page(port);
    std::thread::sleep(Duration::from_secs(6));
    assert!(server.0.try_wait().unwrap().is_none(), "must not stop while a page is open");

    // Closing the page ends the event stream; after the idle time the server exits cleanly.
    drop(page);
    let deadline = Instant::now() + Duration::from_secs(20);
    let status = loop {
        if let Some(s) = server.0.try_wait().unwrap() {
            break s;
        }
        assert!(Instant::now() < deadline, "the server did not stop after the page was closed");
        std::thread::sleep(Duration::from_millis(250));
    };
    assert!(status.success(), "{status:?}");
}

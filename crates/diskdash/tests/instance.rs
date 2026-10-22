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

//! Runs the real `diskdash-helper.exe` without elevation and talks to it over its real named pipe.
//! Opening a volume needs administrator rights, so those calls are expected to fail cleanly here;
//! everything else (pipe ACL, secret, framing, allow-list, error reporting) is exercised for real.

#![cfg(windows)]
#![allow(clippy::unwrap_used)]

use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use diskdash_helper::client::{Client, ClientError};
use diskdash_helper::pipe_client::connect_pipe;
use diskdash_helper::proto::ErrorCode;
use diskdash_helper::{pipe_path, PROTOCOL_VERSION};

const SECRET_HEX: &str = "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff";

fn secret() -> Vec<u8> {
    (0..32).map(|i| u8::from_str_radix(&SECRET_HEX[i * 2..i * 2 + 2], 16).unwrap()).collect()
}

struct Helper {
    child: Child,
    pipe: String,
}

impl Drop for Helper {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn start(tag: &str) -> Helper {
    let pipe = format!(
        "diskdash-{:032x}",
        u128::from(std::process::id()) << 32
            | u128::from(tag.len() as u32) << 8
            | tag.bytes().map(u128::from).sum::<u128>()
    );
    let child = Command::new(env!("CARGO_BIN_EXE_diskdash-helper"))
        .args(["--pipe", &pipe, "--secret", SECRET_HEX])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("start the helper");
    Helper { child, pipe: pipe_path(&pipe) }
}

fn connect(h: &Helper) -> std::fs::File {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        match connect_pipe(&h.pipe) {
            Ok(f) => return f,
            Err(e) if Instant::now() < deadline => {
                let _ = e;
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => panic!("cannot connect to the helper pipe: {e}"),
        }
    }
}

#[test]
fn the_helper_serves_its_owner_and_reports_what_it_cannot_do() {
    let h = start("serves");
    let mut c = Client::connect(connect(&h), &secret()).expect("authenticated");

    let vols = c.volumes().unwrap();
    assert!(!vols.is_empty(), "the helper lists the fixed internal volumes");
    assert!(vols.iter().all(|v| !v.id.is_empty()));

    // Ids that are not the helper's own volumes are refused before anything is opened.
    match c.mft_info(r"C:\Windows") {
        Err(ClientError::Remote { code: ErrorCode::UnknownVolume, .. }) => {}
        other => panic!("{other:?}"),
    }
    match c.smart(9_999) {
        Err(ClientError::Remote { code: ErrorCode::UnknownVolume, .. }) => {}
        other => panic!("{other:?}"),
    }

    // A real NTFS volume: succeeds when the test runs elevated, fails with a clear reason otherwise.
    if let Some(ntfs) = vols.iter().find(|v| v.filesystem.eq_ignore_ascii_case("NTFS")) {
        match c.mft_info(&ntfs.id) {
            Ok(info) => assert!(info.record_count > 0, "elevated run: MFT geometry is real"),
            Err(ClientError::Remote { code: ErrorCode::Failed, message }) => {
                assert!(!message.is_empty(), "the reason is reported, not swallowed")
            }
            other => panic!("{other:?}"),
        }
    }
    // The session survives a failed operation.
    assert_eq!(c.volumes().unwrap().len(), vols.len());
}

#[test]
fn the_wrong_secret_is_rejected_and_the_helper_exits() {
    let mut h = start("wrong");
    let wrong = vec![9u8; 32];
    match Client::connect(connect(&h), &wrong) {
        Err(ClientError::Remote { code: ErrorCode::NotAuthenticated, .. }) => {}
        Err(other) => panic!("{other:?}"),
        Ok(_) => panic!("a wrong secret must not authenticate"),
    }
    // One client per launch: after a rejected one the helper ends and the pipe is gone.
    let deadline = Instant::now() + Duration::from_secs(10);
    while h.child.try_wait().unwrap().is_none() {
        assert!(Instant::now() < deadline, "the helper should exit after serving its one client");
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn bad_command_lines_are_refused_before_anything_is_created() {
    let exe = env!("CARGO_BIN_EXE_diskdash-helper");
    for args in [
        vec!["--pipe", r"..\evil", "--secret", SECRET_HEX],
        vec!["--pipe", "diskdash-0123456789abcdef0123456789abcdef", "--secret", "abcd"],
        vec![],
    ] {
        let status = Command::new(exe).args(&args).stdout(Stdio::null()).stderr(Stdio::null()).status().unwrap();
        assert_eq!(status.code(), Some(2), "{args:?}");
    }
    let _ = PROTOCOL_VERSION;
}

//! `cargo xtask package`: build a portable Windows zip.
//!
//! Builds the UI and the release binaries, stages `diskdash.exe`, the elevated helper and a
//! README, zips them under `dist/`, writes a SHA-256, and smoke-tests the packaged exe by
//! starting it against a throw-away data directory. This is build tooling: it writes
//! only under `dist/` and a temporary directory, never to a user's drives.
#![allow(clippy::disallowed_methods, clippy::disallowed_types)]

use std::fs;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::time::{Duration, Instant};

const README: &str = include_str!("package_readme.txt");
const LAUNCHER: &str = "@echo off\r\nrem Starts diskdash and opens it in your browser. It stops by itself after the page is closed.\r\ncd /d \"%~dp0\"\r\necho diskdash runs on http://127.0.0.1:8765. It stops by itself about 30 seconds after you close its browser page.\r\necho To reopen the page, run this file again.\r\ndiskdash.exe --open --exit-when-idle\r\n";

/// `version = "x.y.z"` from `[workspace.package]`.
pub fn workspace_version(cargo_toml: &str) -> Option<String> {
    let mut in_pkg = false;
    for line in cargo_toml.lines() {
        let l = line.trim();
        if l.starts_with('[') {
            in_pkg = l == "[workspace.package]";
        } else if in_pkg {
            if let Some(v) = l.strip_prefix("version") {
                return v.split('"').nth(1).map(str::to_string);
            }
        }
    }
    None
}

fn run(cmd: &mut Command, what: &str) -> Result<(), String> {
    eprintln!("> {what}");
    match cmd.status() {
        Ok(s) if s.success() => Ok(()),
        Ok(s) => Err(format!("{what} failed ({s})")),
        Err(e) => Err(format!("{what}: {e}")),
    }
}

fn sha256(path: &Path) -> Result<String, String> {
    let out = Command::new("certutil").arg("-hashfile").arg(path).arg("SHA256").output().map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&out.stdout);
    text.lines()
        .nth(1)
        .map(|l| l.replace(' ', "").to_lowercase())
        .filter(|h| h.len() == 64)
        .ok_or_else(|| "cannot compute SHA-256".to_string())
}

/// GET `path` from 127.0.0.1:`port` over a plain socket (no HTTP client dependency).
fn get(port: u16, path: &str) -> Option<String> {
    let mut s = TcpStream::connect(("127.0.0.1", port)).ok()?;
    s.set_read_timeout(Some(Duration::from_secs(3))).ok()?;
    write!(s, "GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n").ok()?;
    let mut body = String::new();
    s.read_to_string(&mut body).ok()?;
    Some(body)
}

/// Start the packaged exe against an empty data directory and check it serves the UI.
fn smoke_test(stage: &Path) -> Result<(), String> {
    let data = std::env::temp_dir().join(format!("diskdash-package-test-{}", std::process::id()));
    let port = 8798;
    let mut child = Command::new(stage.join("diskdash.exe"))
        .args(["--port", &port.to_string()])
        .env("DISKDASH_DATA_DIR", &data)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("cannot start the packaged exe: {e}"))?;
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut result = Err("the packaged exe did not answer within 30 s".to_string());
    while Instant::now() < deadline {
        if let Some(health) = get(port, "/api/v1/health").filter(|h| h.contains("200 OK")) {
            let page = get(port, "/").unwrap_or_default();
            result = if page.contains("<title>")
                && page.contains("diskdash")
                && !page.contains("http://")
                && !page.contains("https://")
            {
                let _ = health;
                Ok(())
            } else {
                Err("the packaged exe answered but did not serve the UI".into())
            };
            break;
        }
        std::thread::sleep(Duration::from_millis(300));
    }
    let _ = child.kill();
    let _ = child.wait();
    let _ = fs::remove_dir_all(&data);
    result
}

pub fn package(root: &Path) -> ExitCode {
    match build(root) {
        Ok(zip) => {
            println!("package: {}", zip.display());
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("package: {e}");
            ExitCode::FAILURE
        }
    }
}

fn build(root: &Path) -> Result<PathBuf, String> {
    let version = workspace_version(&fs::read_to_string(root.join("Cargo.toml")).map_err(|e| e.to_string())?)
        .ok_or("no workspace version")?;
    let frontend = root.join("frontend");
    if !frontend.join("node_modules").exists() {
        return Err("frontend/node_modules is missing: run `npm ci` in frontend/ first".into());
    }
    // Node scripts are called directly (npx/npm shims are not needed for this).
    run(
        Command::new("node").current_dir(&frontend).args(["node_modules/typescript/bin/tsc", "-b"]),
        "type-check the UI",
    )?;
    run(Command::new("node").current_dir(&frontend).args(["node_modules/vite/bin/vite.js", "build"]), "build the UI")?;
    run(
        Command::new("node").current_dir(&frontend).args(["scripts/check-bundle.mjs"]),
        "check the UI bundle has no external URLs",
    )?;
    run(
        Command::new("cargo").current_dir(root).args(["build", "--release", "-p", "diskdash", "-p", "diskdash-helper"]),
        "build the release binaries",
    )?;

    let name = format!("diskdash-{version}-windows-x64");
    let dist = root.join("dist");
    let stage = dist.join(&name);
    if stage.exists() {
        fs::remove_dir_all(&stage).map_err(|e| e.to_string())?;
    }
    fs::create_dir_all(&stage).map_err(|e| e.to_string())?;
    for exe in ["diskdash.exe", "diskdash-helper.exe"] {
        fs::copy(root.join("target/release").join(exe), stage.join(exe)).map_err(|e| format!("{exe}: {e}"))?;
    }
    fs::write(stage.join("README.txt"), README.replace("{version}", &version).replace('\n', "\r\n"))
        .map_err(|e| e.to_string())?;
    fs::write(stage.join("Start diskdash.cmd"), LAUNCHER).map_err(|e| e.to_string())?;

    eprintln!("> smoke-test the packaged exe");
    smoke_test(&stage)?;

    let zip = dist.join(format!("{name}.zip"));
    if zip.exists() {
        fs::remove_file(&zip).map_err(|e| e.to_string())?;
    }
    let ps = format!(
        "Compress-Archive -Path '{}' -DestinationPath '{}' -CompressionLevel Optimal",
        stage.display(),
        zip.display()
    );
    run(Command::new("powershell").args(["-NoProfile", "-Command", &ps]), "create the zip")?;
    let hash = sha256(&zip)?;
    fs::write(dist.join(format!("{name}.zip.sha256")), format!("{hash}  {name}.zip\r\n")).map_err(|e| e.to_string())?;
    let size = fs::metadata(&zip).map_err(|e| e.to_string())?.len();
    eprintln!("> {} MB, sha256 {hash}", size >> 20);
    Ok(zip)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_version_comes_from_the_workspace_package_table() {
        let toml = "[workspace]\nmembers = []\n\n[workspace.package]\nversion = \"1.2.3\"\nedition = \"2021\"\n\n[workspace.dependencies]\nversion = \"9\"\n";
        assert_eq!(workspace_version(toml).as_deref(), Some("1.2.3"));
        assert_eq!(workspace_version("[package]\nversion = \"1\"\n"), None);
    }

    #[test]
    fn the_readme_and_launcher_promise_only_what_the_app_does() {
        assert!(README.contains("{version}") && README.contains("read-only"));
        assert!(LAUNCHER.contains("--open") && !LAUNCHER.to_lowercase().contains("runas"));
    }
}

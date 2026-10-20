//! Repository tasks.
//!
//! - `cargo xtask readonly-check`: fail if any crate (except `test-fixtures`)
//!   references a Win32 API that can modify files, links, ACLs or the registry.
//! - `cargo xtask no-http-client`: fail if the server binary depends on an
//!   HTTP client or TLS crate (safety rule 2: no outbound network).

mod os_integration;
mod package;
mod readonly;

use std::path::PathBuf;
use std::process::{Command, ExitCode};

fn main() -> ExitCode {
    let task = std::env::args().nth(1).unwrap_or_default();
    let root = workspace_root();
    match task.as_str() {
        "readonly-check" => report(readonly::check_tree(&root.join("crates"))),
        "no-http-client" => no_http_client(),
        "no-os-integration" => no_os_integration(),
        "package" => package::package(&root),
        _ => {
            eprintln!("usage: cargo xtask <readonly-check|no-http-client|no-os-integration|package>");
            ExitCode::from(2)
        }
    }
}

fn workspace_root() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest.parent().map(PathBuf::from).unwrap_or(manifest)
}

fn report(result: std::io::Result<Vec<readonly::Violation>>) -> ExitCode {
    match result {
        Ok(v) if v.is_empty() => {
            println!("readonly-check: ok");
            ExitCode::SUCCESS
        }
        Ok(v) => {
            for violation in &v {
                eprintln!("{violation}");
            }
            eprintln!("readonly-check: {} violation(s)", v.len());
            ExitCode::FAILURE
        }
        Err(e) => {
            eprintln!("readonly-check: {e}");
            ExitCode::FAILURE
        }
    }
}

const FORBIDDEN_CRATES: &[&str] =
    &["reqwest", "ureq", "surf", "isahc", "attohttpc", "curl", "hyper-tls", "hyper-rustls", "native-tls", "rustls"];

fn no_http_client() -> ExitCode {
    let output = Command::new("cargo")
        .args(["tree", "-p", "diskdash", "-e", "normal", "--prefix", "none", "--format", "{p}"])
        .output();
    let output = match output {
        Ok(o) if o.status.success() => o,
        Ok(o) => {
            eprintln!("cargo tree failed: {}", String::from_utf8_lossy(&o.stderr));
            return ExitCode::FAILURE;
        }
        Err(e) => {
            eprintln!("cannot run cargo tree: {e}");
            return ExitCode::FAILURE;
        }
    };
    let tree = String::from_utf8_lossy(&output.stdout);
    let found: Vec<&str> = FORBIDDEN_CRATES
        .iter()
        .copied()
        .filter(|name| tree.lines().any(|line| line.split_whitespace().next() == Some(*name)))
        .collect();
    if found.is_empty() {
        println!("no-http-client: ok");
        ExitCode::SUCCESS
    } else {
        eprintln!("no-http-client: forbidden crates in diskdash: {}", found.join(", "));
        ExitCode::FAILURE
    }
}

/// Alerts stay inside the app: no OS notifications, tray icons, shortcuts, services or scheduled tasks.
fn no_os_integration() -> ExitCode {
    let root = workspace_root();
    let mut failed = false;
    match os_integration::check_tree(&root.join("crates")) {
        Ok(v) if v.is_empty() => {}
        Ok(v) => {
            for x in &v {
                eprintln!("{}:{}: `{}` is not allowed (alerts are in-app only)", x.file.display(), x.line, x.what);
            }
            failed = true;
        }
        Err(e) => {
            eprintln!("no-os-integration: {e}");
            return ExitCode::FAILURE;
        }
    }
    for package in ["diskdash", "diskdash-helper"] {
        let out = Command::new("cargo")
            .args(["tree", "-p", package, "-e", "normal", "--prefix", "none", "--format", "{p}"])
            .output();
        match out {
            Ok(o) if o.status.success() => {
                let tree = String::from_utf8_lossy(&o.stdout);
                let found = os_integration::forbidden_crates_in(&tree);
                if !found.is_empty() {
                    eprintln!("no-os-integration: {package} depends on {}", found.join(", "));
                    failed = true;
                }
            }
            _ => {
                eprintln!("no-os-integration: cannot run cargo tree for {package}");
                failed = true;
            }
        }
    }
    if failed {
        ExitCode::FAILURE
    } else {
        println!("no-os-integration: ok");
        ExitCode::SUCCESS
    }
}

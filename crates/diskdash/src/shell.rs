//! "Open in Explorer": show a scanned item in Windows Explorer. This only
//! launches Explorer with `/select,` — it never opens, reads or changes the item.

use std::os::windows::process::CommandExt;
use std::process::Command;

pub trait Launcher: Send + Sync {
    fn reveal(&self, path: &str) -> std::io::Result<()>;
}

/// Starts `explorer.exe /select,"<path>"` (no shell involved).
pub struct ExplorerLauncher;

/// Explorer does not follow MSVCRT quoting, so the argument is passed raw.
pub fn explorer_arg(path: &str) -> String {
    format!("/select,\"{}\"", path.replace('"', ""))
}

impl Launcher for ExplorerLauncher {
    fn reveal(&self, path: &str) -> std::io::Result<()> {
        let windir = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
        Command::new(format!(r"{windir}\explorer.exe")).raw_arg(explorer_arg(path)).spawn().map(|_| ())
    }
}

/// Metadata-only existence check (no handle with data access is opened).
pub fn exists(path: &str) -> bool {
    std::fs::symlink_metadata(diskdash_core::paths::to_extended(path)).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn argument_is_quoted_for_explorer() {
        assert_eq!(explorer_arg(r"C:\My Files\a b.txt"), r#"/select,"C:\My Files\a b.txt""#);
        assert_eq!(explorer_arg(r#"C:\bad"name"#), r#"/select,"C:\badname""#);
    }

    #[test]
    fn existence_check() {
        assert!(exists(r"C:\Windows"));
        assert!(!exists(r"C:\definitely\not\here.txt"));
    }
}

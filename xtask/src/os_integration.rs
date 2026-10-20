//! Guard for "alerts appear only inside the app": diskdash must not use OS
//! notifications, tray icons, Start-menu shortcuts, services or scheduled tasks.
//!
//! Two checks: no dependency on a crate that provides them, and no use of the
//! Win32/WinRT APIs or tools that create them in any crate's source.

use std::fs;
use std::path::{Path, PathBuf};

/// Crates that would bring OS notifications, trays, services or schedulers.
pub const FORBIDDEN_CRATES: &[&str] = &[
    "notify-rust",
    "winrt-notification",
    "tauri-winrt-notification",
    "win-toast-notify",
    "windows-service",
    "tray-icon",
    "trayicon",
    "systray",
    "tray-item",
    "tokio-cron-scheduler",
    "job_scheduler",
    "clokwerk",
    "cron",
];

/// API names that must not appear as identifiers in code.
pub const FORBIDDEN_IDENTS: &[&str] = &[
    "ToastNotification",
    "ToastNotificationManager",
    "Shell_NotifyIconW",
    "Shell_NotifyIconA",
    "NOTIFYICONDATAW",
    "SetCurrentProcessExplicitAppUserModelID",
    "IShellLinkW",
    "ITaskService",
    "ITaskScheduler",
    "CreateServiceW",
    "StartServiceCtrlDispatcherW",
    "RegisterServiceCtrlHandlerW",
    "INotificationActivationCallback",
];

/// Tool names that must not appear in code strings (nor in embedded scripts).
pub const FORBIDDEN_STRINGS: &[&str] =
    &["schtasks", "register-scheduledtask", "new-scheduledtask", "burnttoast", "new-service", "sc.exe create"];

#[derive(Debug, PartialEq, Eq)]
pub struct Violation {
    pub file: PathBuf,
    pub line: usize,
    pub what: String,
}

fn identifiers(line: &str) -> impl Iterator<Item = &str> {
    line.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).filter(|s| !s.is_empty())
}

/// Drop a `//` comment from a line unless the `//` is inside a string.
fn without_line_comment(line: &str) -> &str {
    let b = line.as_bytes();
    let (mut in_str, mut i) = (false, 0);
    while i + 1 < b.len() {
        match b[i] {
            b'\\' if in_str => i += 1,
            b'"' => in_str = !in_str,
            b'/' if !in_str && b[i + 1] == b'/' => return &line[..i],
            _ => {}
        }
        i += 1;
    }
    line
}

pub fn check_source(file: &Path, src: &str) -> Vec<Violation> {
    let mut out = Vec::new();
    let is_script = file.extension().is_some_and(|e| e.eq_ignore_ascii_case("ps1"));
    for (idx, raw) in src.lines().enumerate() {
        let line = if is_script { raw } else { without_line_comment(raw) };
        let lower = line.to_lowercase();
        if !is_script {
            for ident in identifiers(line) {
                if FORBIDDEN_IDENTS.contains(&ident) {
                    out.push(Violation { file: file.to_path_buf(), line: idx + 1, what: ident.to_string() });
                }
            }
        }
        for s in FORBIDDEN_STRINGS {
            if lower.contains(s) && !(is_script && lower.trim_start().starts_with('#')) {
                out.push(Violation { file: file.to_path_buf(), line: idx + 1, what: (*s).to_string() });
            }
        }
    }
    out
}

/// Every `.rs` and `.ps1` file under `crates_dir` (test code and the test-fixtures crate excluded).
pub fn check_tree(crates_dir: &Path) -> std::io::Result<Vec<Violation>> {
    let mut out = Vec::new();
    let mut stack = vec![crates_dir.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir)? {
            let entry = entry?;
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            if entry.file_type()?.is_dir() {
                if matches!(name.as_str(), "test-fixtures" | "target" | "tests") || name.starts_with('.') {
                    continue;
                }
                stack.push(path);
            } else if name.ends_with(".rs") || name.ends_with(".ps1") {
                out.extend(check_source(&path, &fs::read_to_string(&path)?));
            }
        }
    }
    out.sort_by(|a, b| (&a.file, a.line).cmp(&(&b.file, b.line)));
    Ok(out)
}

/// Dependencies from `cargo tree` output (one `name version` per line) that are on the deny list.
pub fn forbidden_crates_in(tree: &str) -> Vec<&'static str> {
    FORBIDDEN_CRATES
        .iter()
        .copied()
        .filter(|name| tree.lines().any(|l| l.split_whitespace().next() == Some(*name)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn what(src: &str) -> Vec<String> {
        check_source(Path::new("x.rs"), src).into_iter().map(|v| v.what).collect()
    }

    #[test]
    fn toast_tray_service_and_task_apis_are_caught() {
        assert_eq!(what("let n = ToastNotificationManager::CreateToastNotifier();"), vec!["ToastNotificationManager"]);
        assert_eq!(what("Shell_NotifyIconW(NIM_ADD, &data);"), vec!["Shell_NotifyIconW"]);
        assert_eq!(
            what("SetCurrentProcessExplicitAppUserModelID(id);"),
            vec!["SetCurrentProcessExplicitAppUserModelID"]
        );
        assert_eq!(what("let t: ITaskService = co_create();"), vec!["ITaskService"]);
        assert_eq!(what("CreateServiceW(scm, name);"), vec!["CreateServiceW"]);
        assert_eq!(what(r#"Command::new("schtasks").args(["/create"]);"#), vec!["schtasks"]);
        assert_eq!(what(r#"run("Register-ScheduledTask -TaskName x")"#), vec!["register-scheduledtask"]);
    }

    #[test]
    fn comments_may_mention_them_but_code_may_not() {
        assert!(what("// no ToastNotification and no schtasks here\nlet x = 1;").is_empty());
        assert!(what("let x = 1; // never call Shell_NotifyIconW").is_empty());
        assert_eq!(
            what(r#"let u = "http://x"; Shell_NotifyIconW();"#),
            vec!["Shell_NotifyIconW"],
            "// inside a string is not a comment"
        );
    }

    #[test]
    fn embedded_powershell_is_scanned_too() {
        let v = check_source(
            Path::new("q.ps1"),
            "# fine: schtasks is only mentioned here\nRegister-ScheduledTask -TaskName x\n",
        );
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].line, 2);
    }

    #[test]
    fn forbidden_crates_are_matched_by_exact_name() {
        let tree = "diskdash v0.1.0\ncronos v1.0.0\nnotify-rust v4.0.0\nserde v1.0.0\n";
        assert_eq!(forbidden_crates_in(tree), vec!["notify-rust"], "`cronos` is not `cron`");
    }

    #[test]
    fn the_real_workspace_is_clean() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().join("crates");
        let v = check_tree(&root).unwrap();
        assert!(v.is_empty(), "{v:?}");
    }
}

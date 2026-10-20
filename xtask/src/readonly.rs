//! Source scan for Win32 APIs that could modify the filesystem, links, ACLs
//! or the registry. Comments and string literals are ignored, so docs may
//! mention the APIs. Clippy covers the `std::fs` side (see `clippy.toml`).

use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

/// Identifiers that must never appear in application code.
pub const DENY_LIST: &[&str] = &[
    // File and directory mutation
    "DeleteFileW",
    "DeleteFileA",
    "RemoveDirectoryW",
    "MoveFileW",
    "MoveFileExW",
    "MoveFileWithProgressW",
    "ReplaceFileW",
    "CopyFileW",
    "CopyFileExW",
    "CopyFile2",
    "CreateDirectoryW",
    "WriteFile",
    "WriteFileEx",
    "SetEndOfFile",
    "SetFileTime",
    "SetFileAttributesW",
    "SetFileInformationByHandle",
    "SetFileValidData",
    "SHFileOperationW",
    "IFileOperation",
    // Links
    "CreateSymbolicLinkW",
    "CreateHardLinkW",
    // ACLs / security
    "SetNamedSecurityInfoW",
    "SetSecurityInfo",
    "SetFileSecurityW",
    // Registry
    "RegSetValueExW",
    "RegSetKeyValueW",
    "RegDeleteKeyW",
    "RegDeleteKeyExW",
    "RegDeleteValueW",
    "RegCreateKeyExW",
    // Write / delete access rights and write IOCTLs
    "GENERIC_WRITE",
    "FILE_GENERIC_WRITE",
    "FILE_WRITE_DATA",
    "FILE_APPEND_DATA",
    "FILE_WRITE_ATTRIBUTES",
    "FILE_WRITE_EA",
    "FILE_DELETE_ON_CLOSE",
    "FILE_FLAG_DELETE_ON_CLOSE",
    "FSCTL_SET_SPARSE",
    "FSCTL_SET_COMPRESSION",
    "FSCTL_SET_REPARSE_POINT",
    "FSCTL_DELETE_REPARSE_POINT",
    "FSCTL_DELETE_USN_JOURNAL",
    "FSCTL_CREATE_USN_JOURNAL",
];

/// The one file allowed to say `GENERIC_WRITE`: the client end of the helper's own named pipe, which is
/// an IPC channel and cannot be used without write access. Nothing else is exempt, in that file or any other.
pub const PIPE_CLIENT_FILE: &str = "diskdash-helper/src/pipe_client.rs";
pub const PIPE_CLIENT_ALLOWED: &[&str] = &["GENERIC_WRITE"];

fn exempt(file: &Path, ident: &str) -> bool {
    let p = file.to_string_lossy().replace('\\', "/");
    p.ends_with(PIPE_CLIENT_FILE) && PIPE_CLIENT_ALLOWED.contains(&ident)
}

#[derive(Debug, PartialEq, Eq)]
pub struct Violation {
    pub file: PathBuf,
    pub line: usize,
    pub ident: String,
}

impl fmt::Display for Violation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}: `{}` is not allowed (diskdash is read-only)", self.file.display(), self.line, self.ident)
    }
}

/// Scan every `.rs` file under `crates_dir`, skipping the `test-fixtures` crate.
pub fn check_tree(crates_dir: &Path) -> std::io::Result<Vec<Violation>> {
    let mut out = Vec::new();
    let mut stack = vec![crates_dir.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir)? {
            let entry = entry?;
            let path = entry.path();
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if entry.file_type()?.is_dir() {
                if name == "test-fixtures" || name == "target" || name.starts_with('.') {
                    continue;
                }
                stack.push(path);
            } else if name.ends_with(".rs") {
                let src = fs::read_to_string(&path)?;
                out.extend(check_source(&path, &src).into_iter().filter(|v| !exempt(&v.file, &v.ident)));
            }
        }
    }
    out.sort_by(|a, b| (&a.file, a.line).cmp(&(&b.file, b.line)));
    Ok(out)
}

/// Scan one source file's text.
pub fn check_source(file: &Path, src: &str) -> Vec<Violation> {
    let code = strip_comments_and_strings(src);
    let mut out = Vec::new();
    for (idx, line) in code.lines().enumerate() {
        for ident in identifiers(line) {
            if DENY_LIST.contains(&ident) {
                out.push(Violation { file: file.to_path_buf(), line: idx + 1, ident: ident.to_string() });
            }
        }
    }
    out
}

fn identifiers(line: &str) -> impl Iterator<Item = &str> {
    line.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).filter(|s| !s.is_empty())
}

/// Replace comments and string/char literals with spaces, keeping newlines so
/// line numbers stay correct.
fn strip_comments_and_strings(src: &str) -> String {
    let chars: Vec<char> = src.chars().collect();
    let mut out = String::with_capacity(src.len());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).copied();
        if c == '/' && next == Some('/') {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
        } else if c == '/' && next == Some('*') {
            let mut depth = 0;
            while i < chars.len() {
                if chars[i] == '/' && chars.get(i + 1) == Some(&'*') {
                    depth += 1;
                    i += 2;
                } else if chars[i] == '*' && chars.get(i + 1) == Some(&'/') {
                    depth -= 1;
                    i += 2;
                    if depth == 0 {
                        break;
                    }
                } else {
                    if chars[i] == '\n' {
                        out.push('\n');
                    }
                    i += 1;
                }
            }
        } else if c == 'r' && (next == Some('"') || next == Some('#')) && is_raw_string_start(&chars, i) {
            i += 1;
            let mut hashes = 0;
            while chars.get(i) == Some(&'#') {
                hashes += 1;
                i += 1;
            }
            i += 1; // opening quote
            while i < chars.len() {
                if chars[i] == '"' && (0..hashes).all(|k| chars.get(i + 1 + k) == Some(&'#')) {
                    i += 1 + hashes;
                    break;
                }
                if chars[i] == '\n' {
                    out.push('\n');
                }
                i += 1;
            }
        } else if c == '"' {
            i += 1;
            while i < chars.len() && chars[i] != '"' {
                if chars[i] == '\\' {
                    i += 1;
                }
                if chars.get(i) == Some(&'\n') {
                    out.push('\n');
                }
                i += 1;
            }
            i += 1;
        } else {
            out.push(c);
            i += 1;
        }
    }
    out
}

fn is_raw_string_start(chars: &[char], i: usize) -> bool {
    // `r"..."` or `r#"..."#`, not an identifier ending in r (e.g. `for`).
    let prev_is_ident = i > 0 && (chars[i - 1].is_ascii_alphanumeric() || chars[i - 1] == '_');
    if prev_is_ident {
        return false;
    }
    let mut j = i + 1;
    while chars.get(j) == Some(&'#') {
        j += 1;
    }
    chars.get(j) == Some(&'"')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn idents(src: &str) -> Vec<String> {
        check_source(Path::new("x.rs"), src).into_iter().map(|v| v.ident).collect()
    }

    #[test]
    fn planted_delete_file_is_caught() {
        let src = "use windows::Win32::Storage::FileSystem::DeleteFileW;\nfn f() { unsafe { DeleteFileW(p) }; }\n";
        let v = check_source(Path::new("x.rs"), src);
        assert_eq!(v.len(), 2);
        assert_eq!(v[1].line, 2);
    }

    #[test]
    fn write_access_is_caught() {
        assert_eq!(idents("let a = GENERIC_WRITE | FILE_SHARE_READ;"), vec!["GENERIC_WRITE"]);
    }

    #[test]
    fn comments_and_strings_are_ignored() {
        let src =
            "// never call DeleteFileW\n/* nor MoveFileExW */\nlet s = \"SetFileTime\";\nlet r = r#\"CopyFileW\"#;\n";
        assert!(idents(src).is_empty());
    }

    #[test]
    fn read_only_apis_pass() {
        let src = "FindFirstFileExW(p, info, data, search, None, flags); GetFileInformationByHandleEx(h);";
        assert!(idents(src).is_empty());
    }

    #[test]
    fn line_numbers_survive_multiline_comments() {
        let src = "/*\n\n*/\nWriteFile(h);\n";
        let v = check_source(Path::new("x.rs"), src);
        assert_eq!(v[0].line, 4);
    }

    #[test]
    fn only_the_pipe_client_may_ask_for_write_access_on_a_handle() {
        assert!(exempt(Path::new("crates/diskdash-helper/src/pipe_client.rs"), "GENERIC_WRITE"));
        assert!(exempt(Path::new(r"D:\p\crates\diskdash-helper\src\pipe_client.rs"), "GENERIC_WRITE"));
        // Not in other files, and not for any other identifier, even in that file.
        assert!(!exempt(Path::new("crates/diskdash/src/anything.rs"), "GENERIC_WRITE"));
        assert!(!exempt(Path::new("crates/diskdash-helper/src/win.rs"), "GENERIC_WRITE"));
        assert!(!exempt(Path::new("crates/diskdash-helper/src/pipe_client.rs"), "DeleteFileW"));
        assert!(!exempt(Path::new("crates/diskdash-helper/src/pipe_client.rs"), "FILE_WRITE_DATA"));
    }

    #[test]
    fn test_fixtures_crate_is_skipped() {
        #[allow(clippy::disallowed_methods)]
        fn setup(root: &Path) {
            std::fs::create_dir_all(root.join("test-fixtures/src")).unwrap();
            std::fs::create_dir_all(root.join("app/src")).unwrap();
            std::fs::write(root.join("test-fixtures/src/lib.rs"), "CreateHardLinkW();").unwrap();
            std::fs::write(root.join("app/src/lib.rs"), "fn ok() {}").unwrap();
        }
        let dir = tempfile::tempdir().unwrap();
        setup(dir.path());
        assert!(check_tree(dir.path()).unwrap().is_empty());
    }
}

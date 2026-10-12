//! Runs fixed external commands (PowerShell, fsutil, smartctl) with a timeout.
//! Behind a trait so tests never start real processes.

use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use base64::Engine;

pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug, thiserror::Error)]
pub enum RunError {
    #[error("cannot start {program}: {source}")]
    Spawn { program: String, source: std::io::Error },
    #[error("{program} timed out after {secs} s")]
    Timeout { program: String, secs: u64 },
    #[error("{program} exited with {code:?}: {stderr}")]
    Failed { program: String, code: Option<i32>, stdout: String, stderr: String },
}

pub trait CommandRunner: Send + Sync {
    /// Run `program args…`, returning stdout.
    fn run(&self, program: &Path, args: &[String]) -> Result<String, RunError>;
}

/// Real process runner with a hard timeout.
pub struct ProcessRunner {
    pub timeout: Duration,
}

impl Default for ProcessRunner {
    fn default() -> Self {
        Self { timeout: DEFAULT_TIMEOUT }
    }
}

impl CommandRunner for ProcessRunner {
    fn run(&self, program: &Path, args: &[String]) -> Result<String, RunError> {
        let name = program.display().to_string();
        let mut child = Command::new(program)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|source| RunError::Spawn { program: name.clone(), source })?;

        // Drain pipes on threads so a chatty process cannot block on a full pipe.
        let mut out_pipe = child.stdout.take();
        let mut err_pipe = child.stderr.take();
        let out_t = std::thread::spawn(move || {
            let mut s = Vec::new();
            if let Some(p) = out_pipe.as_mut() {
                let _ = p.read_to_end(&mut s);
            }
            s
        });
        let err_t = std::thread::spawn(move || {
            let mut s = Vec::new();
            if let Some(p) = err_pipe.as_mut() {
                let _ = p.read_to_end(&mut s);
            }
            s
        });

        let start = Instant::now();
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) if start.elapsed() >= self.timeout => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(RunError::Timeout { program: name, secs: self.timeout.as_secs() });
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(25)),
                Err(source) => return Err(RunError::Spawn { program: name, source }),
            }
        };
        let stdout = String::from_utf8_lossy(&out_t.join().unwrap_or_default()).into_owned();
        let stderr = String::from_utf8_lossy(&err_t.join().unwrap_or_default()).into_owned();
        if status.success() {
            Ok(stdout)
        } else {
            Err(RunError::Failed { program: name, code: status.code(), stdout, stderr: stderr.trim().to_string() })
        }
    }
}

/// Arguments for running an embedded script with Windows PowerShell 5.1.
/// `-EncodedCommand` avoids any quoting of the script text.
pub fn powershell_args(script: &str) -> Vec<String> {
    let utf16: Vec<u8> = script.encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
    let encoded = base64::engine::general_purpose::STANDARD.encode(utf16);
    vec![
        "-NoProfile".into(),
        "-NonInteractive".into(),
        "-ExecutionPolicy".into(),
        "Bypass".into(),
        "-EncodedCommand".into(),
        encoded,
    ]
}

pub fn powershell_exe() -> std::path::PathBuf {
    let root = std::env::var_os("SystemRoot").unwrap_or_else(|| r"C:\Windows".into());
    Path::new(&root).join(r"System32\WindowsPowerShell\v1.0\powershell.exe")
}

pub fn fsutil_exe() -> std::path::PathBuf {
    let root = std::env::var_os("SystemRoot").unwrap_or_else(|| r"C:\Windows".into());
    Path::new(&root).join(r"System32\fsutil.exe")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encoded_command_round_trips() {
        let args = powershell_args("Write-Output 'é'");
        let decoded = base64::engine::general_purpose::STANDARD.decode(&args[5]).unwrap();
        let utf16: Vec<u16> = decoded.chunks(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        assert_eq!(String::from_utf16(&utf16).unwrap(), "Write-Output 'é'");
    }

    #[test]
    fn real_runner_times_out() {
        let r = ProcessRunner { timeout: Duration::from_millis(300) };
        let ping = Path::new(r"C:\Windows\System32\PING.EXE");
        let start = Instant::now();
        let err = r.run(ping, &["-n".into(), "30".into(), "127.0.0.1".into()]).unwrap_err();
        assert!(matches!(err, RunError::Timeout { .. }), "{err}");
        assert!(start.elapsed() < Duration::from_secs(3));
    }

    #[test]
    fn real_runner_captures_stdout() {
        let r = ProcessRunner::default();
        let out = r.run(&powershell_exe(), &powershell_args("Write-Output 42")).unwrap();
        assert_eq!(out.trim(), "42");
    }
}

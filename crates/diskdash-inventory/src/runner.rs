//! Runs fixed external commands (PowerShell, fsutil, smartctl) with a timeout.
//! Behind a trait so tests never start real processes.

use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

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

/// Arguments for running an embedded script with Windows PowerShell 5.1: a plain `-Command`.
/// Deliberately no `-EncodedCommand` or `-ExecutionPolicy Bypass` (antivirus products flag those
/// on sight; execution policy only applies to script files anyway). The scripts contain no double
/// quotes, so the command line needs no escaping. Piping the script through stdin is not an
/// option: PowerShell silently drops multi-line blocks read that way.
pub fn powershell_args(script: &str) -> Vec<String> {
    vec!["-NoProfile".into(), "-NonInteractive".into(), "-Command".into(), script.into()]
}

/// Run an embedded script through PowerShell.
pub fn run_powershell(runner: &dyn CommandRunner, script: &str) -> Result<String, RunError> {
    runner.run(&powershell_exe(), &powershell_args(script))
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
    fn embedded_scripts_need_no_quote_escaping() {
        assert!(!crate::INVENTORY_SCRIPT.contains('"') && !crate::RELIABILITY_SCRIPT.contains('"'));
    }

    #[test]
    fn command_line_has_no_flagged_switches() {
        let line = powershell_args("Get-Date").join(" ").to_lowercase();
        assert!(!line.contains("encodedcommand") && !line.contains("bypass") && !line.contains("hidden"), "{line}");
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
        let out = run_powershell(&r, "Write-Output 'é'; Write-Output 42").unwrap();
        assert_eq!(out.trim().lines().last().unwrap().trim(), "42");
    }

    #[test]
    fn inventory_script_runs_from_stdin_and_parses() {
        let out = run_powershell(&ProcessRunner::default(), crate::INVENTORY_SCRIPT).unwrap();
        let raw = crate::raw::RawInventory::from_json(out.trim()).unwrap();
        assert!(!raw.volumes.is_empty());
    }
}

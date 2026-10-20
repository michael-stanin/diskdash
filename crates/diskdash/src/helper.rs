//! The server's side of the elevated helper: starting it through UAC, keeping the
//! connection, and exposing the few things scans need (MFT entries, USN changes).
//!
//! The server itself stays non-elevated. The helper is started only when the user
//! clicks "Enable elevated features"; declining the prompt leaves everything in
//! reduced mode (directory walk).

use std::fs::File;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, Weak};

use diskdash_core::ntfs::model::{MftEntry, MftError, MftInfo, MftSource, ScanTally};
use diskdash_core::ntfs::usn::{JournalState, StoredUsn};
use diskdash_helper::client::{Client, ClientError};
use diskdash_helper::proto::{HelperVolume, UsnOutcome};
use serde::Serialize;
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "kebab-case")]
#[ts(export)]
pub enum HelperState {
    /// `diskdash-helper.exe` is not next to `diskdash.exe`.
    Unavailable,
    NotRunning,
    /// Waiting for the administrator prompt.
    Starting,
    Running,
    /// The last start failed or the helper stopped; see `message`.
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct HelperStatus {
    pub state: HelperState,
    pub message: Option<String>,
}

impl HelperStatus {
    pub fn running(&self) -> bool {
        self.state == HelperState::Running
    }

    /// Why a scan cannot use the helper right now (shown in the scan dialog).
    pub fn why_not(&self) -> String {
        match (self.state, &self.message) {
            (HelperState::Running, _) => String::new(),
            (HelperState::Unavailable, _) => {
                "the elevated helper (diskdash-helper.exe) is not installed next to diskdash.exe".into()
            }
            (HelperState::Starting, _) => "the administrator prompt is still open".into(),
            (HelperState::Failed, Some(m)) => format!("the elevated helper is not available: {m}"),
            _ => "fast scan requires administrator rights".into(),
        }
    }
}

/// A scan the helper is serving: an [`MftSource`] plus the journal position taken when it started.
pub trait HelperScan: Send {
    fn source(&mut self) -> &mut dyn MftSource;
    fn journal(&self) -> Option<JournalState>;
}

/// What the server needs from the helper. A fake implements it in tests.
pub trait HelperLink: Send + Sync {
    fn status(&self) -> HelperStatus;
    /// Start the helper through UAC and connect to it. Blocks until the user answers the prompt.
    fn enable(&self) -> Result<(), String>;
    fn volumes(&self) -> Result<Vec<HelperVolume>, String>;
    fn open_scan(&self, volume_id: &str) -> Result<Box<dyn HelperScan>, String>;
    fn usn_read(&self, volume_id: &str, stored: Option<StoredUsn>, total_records: u64) -> Result<UsnOutcome, String>;
}

/// Used when no helper exists (tests, non-Windows builds).
pub struct NoHelper;

impl HelperLink for NoHelper {
    fn status(&self) -> HelperStatus {
        HelperStatus { state: HelperState::NotRunning, message: None }
    }

    fn enable(&self) -> Result<(), String> {
        Err("no elevated helper is configured".into())
    }

    fn volumes(&self) -> Result<Vec<HelperVolume>, String> {
        Err("the elevated helper is not running".into())
    }

    fn open_scan(&self, _: &str) -> Result<Box<dyn HelperScan>, String> {
        Err("the elevated helper is not running".into())
    }

    fn usn_read(&self, _: &str, _: Option<StoredUsn>, _: u64) -> Result<UsnOutcome, String> {
        Err("the elevated helper is not running".into())
    }
}

// ---------------------------------------------------------------------------
// The real helper process
// ---------------------------------------------------------------------------

#[cfg(windows)]
mod proc {
    use std::path::Path;

    use windows::core::{HSTRING, PCWSTR};
    use windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0};
    use windows::Win32::System::Threading::{GetExitCodeProcess, WaitForSingleObject};
    use windows::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW};
    use windows::Win32::UI::WindowsAndMessaging::SW_HIDE;

    const ERROR_CANCELLED: i32 = 1223;

    /// A started process we can ask whether it is still alive.
    pub struct Proc(HANDLE);

    // SAFETY: a process handle may be used from any thread.
    unsafe impl Send for Proc {}

    impl Drop for Proc {
        fn drop(&mut self) {
            // SAFETY: handle from ShellExecuteExW, closed once.
            let _ = unsafe { CloseHandle(self.0) };
        }
    }

    impl Proc {
        /// Exit code if the process has ended.
        pub fn exited(&self) -> Option<u32> {
            // SAFETY: waits zero milliseconds on our own handle.
            if unsafe { WaitForSingleObject(self.0, 0) } != WAIT_OBJECT_0 {
                return None;
            }
            let mut code = 0u32;
            // SAFETY: out pointer is a local.
            let _ = unsafe { GetExitCodeProcess(self.0, &mut code) };
            Some(code)
        }
    }

    /// Start `exe` elevated (the `runas` verb shows the UAC prompt) with no visible window.
    pub fn launch_elevated(exe: &Path, args: &str) -> Result<Proc, String> {
        let file = HSTRING::from(exe.as_os_str());
        let params = HSTRING::from(args);
        let verb = HSTRING::from("runas");
        let mut info = SHELLEXECUTEINFOW {
            cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
            fMask: SEE_MASK_NOCLOSEPROCESS,
            lpVerb: PCWSTR(verb.as_ptr()),
            lpFile: PCWSTR(file.as_ptr()),
            lpParameters: PCWSTR(params.as_ptr()),
            nShow: SW_HIDE.0,
            ..Default::default()
        };
        // SAFETY: `info` and the strings it points to live until the call returns.
        match unsafe { ShellExecuteExW(&mut info) } {
            Ok(()) if !info.hProcess.is_invalid() => Ok(Proc(info.hProcess)),
            Ok(()) => Err("Windows did not return the helper process".into()),
            Err(e) if e.code().0 & 0xFFFF == ERROR_CANCELLED => {
                Err("The administrator prompt was declined, so fast scans stay off.".into())
            }
            Err(e) => Err(format!("could not start the elevated helper: {e}")),
        }
    }
}

#[cfg(not(windows))]
mod proc {
    use std::path::Path;

    pub struct Proc;

    impl Proc {
        pub fn exited(&self) -> Option<u32> {
            Some(0)
        }
    }

    pub fn launch_elevated(_: &Path, _: &str) -> Result<Proc, String> {
        Err("the elevated helper is available on Windows only".into())
    }
}

struct Link {
    client: Client<File>,
    proc: proc::Proc,
}

#[derive(Default)]
struct Flags {
    starting: bool,
    failed: Option<String>,
}

/// The real thing: `diskdash-helper.exe` started through UAC, reached over a named pipe.
pub struct ProcessHelper {
    exe: PathBuf,
    flags: Mutex<Flags>,
    link: Mutex<Option<Link>>,
    me: Weak<ProcessHelper>,
}

impl ProcessHelper {
    /// Looks for the helper next to the running executable.
    pub fn beside_current_exe() -> Arc<Self> {
        let exe = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.join("diskdash-helper.exe")))
            .unwrap_or_else(|| PathBuf::from("diskdash-helper.exe"));
        Self::new(exe)
    }

    pub fn new(exe: PathBuf) -> Arc<Self> {
        Arc::new_cyclic(|me| Self { exe, flags: Mutex::new(Flags::default()), link: Mutex::new(None), me: me.clone() })
    }

    fn set_failed(&self, message: String) {
        tracing::warn!(%message, "elevated helper");
        if let Ok(mut f) = self.flags.lock() {
            f.failed = Some(message);
        }
    }

    /// Run one exchange; a dead helper or broken pipe clears the link and records why.
    fn with_client<T>(&self, f: impl FnOnce(&mut Client<File>) -> Result<T, ClientError>) -> Result<T, String> {
        let mut guard = self.link.lock().map_err(|_| "helper lock poisoned".to_string())?;
        let Some(link) = guard.as_mut() else { return Err("the elevated helper is not running".into()) };
        if let Some(code) = link.proc.exited() {
            *guard = None;
            let msg = format!("the elevated helper stopped (exit code {code})");
            self.set_failed(msg.clone());
            return Err(msg);
        }
        match f(&mut link.client) {
            Ok(v) => Ok(v),
            Err(ClientError::Remote { message, .. }) => Err(message),
            Err(e) => {
                *guard = None;
                let msg = format!("lost the connection to the elevated helper: {e}");
                self.set_failed(msg.clone());
                Err(msg)
            }
        }
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

impl HelperLink for ProcessHelper {
    fn status(&self) -> HelperStatus {
        if !self.exe.exists() {
            return HelperStatus { state: HelperState::Unavailable, message: None };
        }
        if let Ok(mut guard) = self.link.lock() {
            if let Some(link) = guard.as_ref() {
                if let Some(code) = link.proc.exited() {
                    *guard = None;
                    drop(guard);
                    self.set_failed(format!("the elevated helper stopped (exit code {code})"));
                } else {
                    return HelperStatus { state: HelperState::Running, message: None };
                }
            }
        }
        let f = self.flags.lock().map(|f| (f.starting, f.failed.clone())).unwrap_or((false, None));
        match f {
            (true, _) => HelperStatus { state: HelperState::Starting, message: None },
            (false, Some(m)) => HelperStatus { state: HelperState::Failed, message: Some(m) },
            _ => HelperStatus { state: HelperState::NotRunning, message: None },
        }
    }

    fn enable(&self) -> Result<(), String> {
        if self.status().running() {
            return Ok(());
        }
        if !self.exe.exists() {
            return Err(format!("{} was not found", self.exe.display()));
        }
        if let Ok(mut f) = self.flags.lock() {
            if f.starting {
                return Err("the administrator prompt is already open".into());
            }
            f.starting = true;
            f.failed = None;
        }
        let result = self.start();
        if let Ok(mut f) = self.flags.lock() {
            f.starting = false;
            if let Err(e) = &result {
                f.failed = Some(e.clone());
            }
        }
        result
    }

    fn volumes(&self) -> Result<Vec<HelperVolume>, String> {
        self.with_client(|c| c.volumes())
    }

    fn open_scan(&self, volume_id: &str) -> Result<Box<dyn HelperScan>, String> {
        let info = self.with_client(|c| c.mft_info(volume_id))?;
        let helper = self.me.upgrade().ok_or("the helper is shutting down")?;
        Ok(Box::new(RemoteScan { helper, volume_id: volume_id.to_string(), info, journal: None }))
    }

    fn usn_read(&self, volume_id: &str, stored: Option<StoredUsn>, total_records: u64) -> Result<UsnOutcome, String> {
        self.with_client(|c| c.usn_read(volume_id, stored, total_records))
    }
}

impl ProcessHelper {
    fn start(&self) -> Result<(), String> {
        use rand::RngCore;
        let mut secret = [0u8; 32];
        let mut name = [0u8; 16];
        rand::thread_rng().fill_bytes(&mut secret);
        rand::thread_rng().fill_bytes(&mut name);
        let pipe = format!("diskdash-{}", hex(&name));
        let args = format!("--pipe {pipe} --secret {}", hex(&secret));
        let process = proc::launch_elevated(&self.exe, &args)?;
        let path = diskdash_helper::pipe_path(&pipe);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        let file = loop {
            if let Some(code) = process.exited() {
                return Err(format!("the elevated helper exited right away (code {code}); see the diskdash log"));
            }
            match connect(&path) {
                Ok(f) => break f,
                Err(e) if std::time::Instant::now() < deadline => {
                    let _ = e;
                    std::thread::sleep(std::time::Duration::from_millis(100));
                }
                Err(e) => return Err(format!("could not connect to the elevated helper: {e}")),
            }
        };
        let client =
            Client::connect(file, &secret).map_err(|e| format!("the elevated helper refused the connection: {e}"))?;
        *self.link.lock().map_err(|_| "helper lock poisoned".to_string())? = Some(Link { client, proc: process });
        Ok(())
    }
}

#[cfg(windows)]
fn connect(path: &str) -> std::io::Result<File> {
    diskdash_helper::pipe_client::connect_pipe(path)
}

#[cfg(not(windows))]
fn connect(_: &str) -> std::io::Result<File> {
    Err(std::io::Error::other("Windows only"))
}

/// Streams entries from the helper, one request per pass.
struct RemoteScan {
    helper: Arc<ProcessHelper>,
    volume_id: String,
    info: MftInfo,
    journal: Option<JournalState>,
}

impl MftSource for RemoteScan {
    fn info(&self) -> &MftInfo {
        &self.info
    }

    fn for_each(&mut self, f: &mut dyn FnMut(MftEntry) -> bool) -> Result<ScanTally, MftError> {
        let id = self.volume_id.clone();
        let (tally, journal) = self.helper.with_client(|c| c.mft_scan(&id, f)).map_err(MftError::Read)?;
        self.journal = journal.or(self.journal);
        Ok(tally)
    }
}

impl HelperScan for RemoteScan {
    fn source(&mut self) -> &mut dyn MftSource {
        self
    }

    fn journal(&self) -> Option<JournalState> {
        self.journal
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_explains_why_the_helper_cannot_be_used() {
        let s = |state, m: Option<&str>| HelperStatus { state, message: m.map(String::from) };
        assert_eq!(s(HelperState::NotRunning, None).why_not(), "fast scan requires administrator rights");
        assert!(s(HelperState::Unavailable, None).why_not().contains("not installed"));
        assert!(s(HelperState::Failed, Some("declined")).why_not().contains("declined"));
        assert!(s(HelperState::Running, None).why_not().is_empty());
    }

    #[test]
    fn a_missing_helper_executable_is_reported_as_unavailable_and_cannot_be_enabled() {
        let h = ProcessHelper::new(PathBuf::from(r"C:\definitely\missing\diskdash-helper.exe"));
        assert_eq!(h.status().state, HelperState::Unavailable);
        assert!(h.enable().unwrap_err().contains("was not found"));
        assert!(h.volumes().is_err());
    }

    #[test]
    fn hex_encoding() {
        assert_eq!(hex(&[0, 15, 255]), "000fff");
    }
}

//! Drive inventory and health for diskdash.
//!
//! Strictly read-only: runs fixed query scripts (PowerShell/CIM), `fsutil
//! behavior query`, and optionally smartctl with an argument allow-list.
//! Fixed internal drives only; network shares are never contacted.

pub mod apps;
pub mod assemble;
pub mod classify;
pub mod health;
pub mod lowspace;
pub mod model;
pub mod native;
pub mod raw;
pub mod runner;
pub mod smartctl;

use std::path::PathBuf;

use assemble::HealthInputs;
use model::{HealthSources, Inventory, TrimState};
use raw::{RawInventory, RawReliability};
use runner::{fsutil_exe, powershell_args, powershell_exe, CommandRunner, RunError};

pub const INVENTORY_SCRIPT: &str = include_str!("../scripts/inventory.ps1");
pub const RELIABILITY_SCRIPT: &str = include_str!("../scripts/reliability.ps1");

#[derive(Debug, thiserror::Error)]
pub enum CollectError {
    #[error(transparent)]
    Run(#[from] RunError),
    #[error("cannot parse inventory output: {0}")]
    Parse(#[from] serde_json::Error),
}

/// Gathers the inventory. Elevated sources are used only when `elevated`.
pub struct Collector<'a> {
    pub runner: &'a dyn CommandRunner,
    pub elevated: bool,
    pub smartctl: Option<PathBuf>,
    pub use_native_serials: bool,
}

impl Collector<'_> {
    /// Run the PowerShell query, falling back to native Win32 if it fails.
    pub fn raw(&self) -> (RawInventory, bool) {
        match self.powershell_raw() {
            Ok(raw) if !raw.volumes.is_empty() => (raw, false),
            Ok(_) => {
                tracing::warn!("inventory query returned no volumes; using native fallback");
                (native::collect(), true)
            }
            Err(e) => {
                tracing::warn!(error = %e, "inventory query failed; using native fallback");
                (native::collect(), true)
            }
        }
    }

    fn powershell_raw(&self) -> Result<RawInventory, CollectError> {
        let out = self.runner.run(&powershell_exe(), &powershell_args(INVENTORY_SCRIPT))?;
        let mut raw = RawInventory::from_json(out.trim())?;
        if raw.logical.is_empty() {
            raw.logical = native::drive_types();
        }
        Ok(raw)
    }

    fn reliability(&self) -> Vec<RawReliability> {
        if !self.elevated {
            return Vec::new();
        }
        let parsed = self
            .runner
            .run(&powershell_exe(), &powershell_args(RELIABILITY_SCRIPT))
            .map_err(CollectError::from)
            .and_then(|out| {
                let trimmed = out.trim();
                if trimmed.is_empty() {
                    return Ok(Vec::new());
                }
                // A single object serialises without the array brackets.
                if trimmed.starts_with('[') {
                    Ok(serde_json::from_str(trimmed)?)
                } else {
                    Ok(vec![serde_json::from_str(trimmed)?])
                }
            });
        parsed.unwrap_or_else(|e| {
            tracing::warn!(error = %e, "reliability counters unavailable");
            Vec::new()
        })
    }

    fn smart(&self) -> Vec<smartctl::SmartData> {
        match (&self.smartctl, self.elevated) {
            (Some(exe), true) => smartctl::collect(self.runner, exe).unwrap_or_else(|e| {
                tracing::warn!(error = %e, "smartctl failed");
                Vec::new()
            }),
            _ => Vec::new(),
        }
    }

    pub fn trim(&self) -> TrimState {
        let args = vec!["behavior".to_string(), "query".to_string(), "DisableDeleteNotify".to_string()];
        match self.runner.run(&fsutil_exe(), &args) {
            Ok(out) => health::parse_trim(&out),
            Err(e) => TrimState { ntfs: None, refs: None, reason: Some(format!("fsutil failed: {e}")) },
        }
    }

    pub fn collect(&self) -> Inventory {
        let (raw, fallback) = self.raw();
        let reliability = self.reliability();
        let smart = self.smart();
        let windows_letter =
            std::env::var("SystemRoot").ok().and_then(|w| w.chars().next()).map(|c| c.to_ascii_uppercase().to_string());
        let inputs = HealthInputs { elevated: self.elevated, reliability: &reliability, smart: &smart, windows_letter };
        let (mut drives, excluded) = assemble::assemble(&raw, &inputs);
        if self.use_native_serials {
            for d in &mut drives {
                d.serial = native::volume_serial(&d.volume_path);
            }
        }
        Inventory {
            drives,
            excluded,
            trim: self.trim(),
            sources: HealthSources {
                elevated: self.elevated,
                smartctl: self.smartctl.as_ref().map(|p| p.display().to_string()),
            },
            fallback,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use std::sync::Mutex;

    /// Replays canned outputs and records what was run.
    struct FakeRunner {
        inventory: Result<String, ()>,
        calls: Mutex<Vec<String>>,
    }

    impl CommandRunner for FakeRunner {
        fn run(&self, program: &Path, args: &[String]) -> Result<String, RunError> {
            let name = program.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
            self.calls.lock().unwrap().push(format!("{name} {}", args.first().cloned().unwrap_or_default()));
            match name.as_str() {
                "powershell.exe" => self.inventory.clone().map_err(|()| RunError::Timeout { program: name, secs: 15 }),
                "fsutil.exe" => Ok("NTFS DisableDeleteNotify = 0  (Disabled)\n".into()),
                _ => {
                    Err(RunError::Failed { program: name, code: Some(1), stdout: String::new(), stderr: String::new() })
                }
            }
        }
    }

    #[test]
    fn collects_from_powershell_without_elevated_calls() {
        let fake = FakeRunner {
            inventory: Ok(include_str!("../tests/fixtures/this_machine.json").to_string()),
            calls: Mutex::new(Vec::new()),
        };
        let c = Collector {
            runner: &fake,
            elevated: false,
            smartctl: Some("smartctl.exe".into()),
            use_native_serials: false,
        };
        let inv = c.collect();
        assert!(!inv.fallback);
        assert_eq!(inv.drives.len(), 3);
        assert_eq!(inv.trim.ntfs, Some(true));
        let calls = fake.calls.lock().unwrap();
        assert_eq!(
            calls.iter().filter(|c| c.starts_with("powershell.exe")).count(),
            1,
            "no reliability call when not elevated"
        );
        assert!(!calls.iter().any(|c| c.starts_with("smartctl")), "smartctl only when elevated");
    }

    #[test]
    fn powershell_timeout_falls_back_to_native() {
        let fake = FakeRunner { inventory: Err(()), calls: Mutex::new(Vec::new()) };
        let c = Collector { runner: &fake, elevated: false, smartctl: None, use_native_serials: false };
        let inv = c.collect();
        assert!(inv.fallback);
        assert!(!inv.drives.is_empty(), "native fallback still finds the Windows drive");
    }
}

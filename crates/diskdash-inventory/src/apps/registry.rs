//! Read-only registry access behind a trait, so the readers can be tested
//! with a fake. The Windows implementation only opens keys with `KEY_READ`.

use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Hive {
    Hklm,
    Hkcu,
}

/// Which registry view to read (matters for HKLM on 64-bit Windows).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum View {
    V64,
    V32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegValue {
    Dword(u32),
    Str(String),
    Binary(Vec<u8>),
}

impl RegValue {
    pub fn as_str(&self) -> Option<&str> {
        match self {
            RegValue::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_dword(&self) -> Option<u32> {
        match self {
            RegValue::Dword(d) => Some(*d),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RegError {
    #[error("key not found")]
    NotFound,
    #[error("access denied")]
    AccessDenied,
    #[error("{0}")]
    Other(String),
}

pub trait Registry {
    fn subkeys(&self, hive: Hive, view: View, path: &str) -> Result<Vec<String>, RegError>;
    fn values(&self, hive: Hive, view: View, path: &str) -> Result<HashMap<String, RegValue>, RegError>;
}

type KeyResult = Result<HashMap<String, RegValue>, RegError>;

/// In-memory registry for tests.
#[derive(Debug, Default, Clone)]
pub struct FakeRegistry {
    keys: HashMap<(Hive, View, String), KeyResult>,
}

fn norm(path: &str) -> String {
    path.trim_matches('\\').to_lowercase()
}

impl FakeRegistry {
    pub fn key(mut self, hive: Hive, view: View, path: &str, values: &[(&str, RegValue)]) -> Self {
        let map = values.iter().map(|(k, v)| ((*k).to_string(), v.clone())).collect();
        self.keys.insert((hive, view, norm(path)), Ok(map));
        self
    }

    pub fn denied(mut self, hive: Hive, view: View, path: &str) -> Self {
        self.keys.insert((hive, view, norm(path)), Err(RegError::AccessDenied));
        self
    }
}

impl Registry for FakeRegistry {
    fn subkeys(&self, hive: Hive, view: View, path: &str) -> Result<Vec<String>, RegError> {
        let prefix = format!("{}\\", norm(path));
        let mut out: Vec<String> = Vec::new();
        let mut found = false;
        for ((h, v, p), r) in &self.keys {
            if *h != hive || *v != view {
                continue;
            }
            if p == &norm(path) {
                if let Err(e) = r {
                    return Err(e.clone());
                }
                found = true;
            }
            if let Some(rest) = p.strip_prefix(&prefix) {
                found = true;
                let first = rest.split('\\').next().unwrap_or("");
                if !out.iter().any(|o| o == first) {
                    out.push(first.to_string());
                }
            }
        }
        if found {
            out.sort();
            Ok(out)
        } else {
            Err(RegError::NotFound)
        }
    }

    fn values(&self, hive: Hive, view: View, path: &str) -> Result<HashMap<String, RegValue>, RegError> {
        self.keys.get(&(hive, view, norm(path))).cloned().unwrap_or(Err(RegError::NotFound))
    }
}

/// The real registry (read-only).
pub struct WindowsRegistry;

mod win {
    use super::{Hive, RegError, RegValue, View};
    use std::collections::HashMap;
    use windows::core::{HSTRING, PWSTR};
    use windows::Win32::Foundation::{
        ERROR_ACCESS_DENIED, ERROR_FILE_NOT_FOUND, ERROR_MORE_DATA, ERROR_NO_MORE_ITEMS, ERROR_SUCCESS, WIN32_ERROR,
    };
    use windows::Win32::System::Registry::{
        RegCloseKey, RegEnumKeyExW, RegEnumValueW, RegOpenKeyExW, HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE,
        KEY_READ, KEY_WOW64_32KEY, KEY_WOW64_64KEY, REG_BINARY, REG_DWORD, REG_EXPAND_SZ, REG_SZ,
    };

    fn map(e: WIN32_ERROR) -> RegError {
        if e == ERROR_FILE_NOT_FOUND {
            RegError::NotFound
        } else if e == ERROR_ACCESS_DENIED {
            RegError::AccessDenied
        } else {
            RegError::Other(format!("registry error {}", e.0))
        }
    }

    /// An open key, closed on drop.
    pub struct Key(HKEY);

    impl Key {
        pub fn open(hive: Hive, view: View, path: &str) -> Result<Key, RegError> {
            let root = match hive {
                Hive::Hklm => HKEY_LOCAL_MACHINE,
                Hive::Hkcu => HKEY_CURRENT_USER,
            };
            let wow = match view {
                View::V64 => KEY_WOW64_64KEY,
                View::V32 => KEY_WOW64_32KEY,
            };
            let mut out = HKEY::default();
            // SAFETY: `out` is a valid out-pointer and the path is a NUL-terminated string
            // that outlives the call. The key is opened for reading only.
            let err = unsafe { RegOpenKeyExW(root, &HSTRING::from(path), 0, KEY_READ | wow, &mut out) };
            if err == ERROR_SUCCESS {
                Ok(Key(out))
            } else {
                Err(map(err))
            }
        }

        pub fn subkeys(&self) -> Result<Vec<String>, RegError> {
            let mut out = Vec::new();
            let mut buf = vec![0u16; 512];
            for index in 0.. {
                let mut len = buf.len() as u32;
                // SAFETY: `buf` holds `len` UTF-16 units and `len` is updated by the call.
                let err = unsafe {
                    RegEnumKeyExW(self.0, index, PWSTR(buf.as_mut_ptr()), &mut len, None, PWSTR::null(), None, None)
                };
                if err == ERROR_NO_MORE_ITEMS {
                    break;
                }
                if err != ERROR_SUCCESS {
                    return Err(map(err));
                }
                out.push(String::from_utf16_lossy(&buf[..len as usize]));
            }
            Ok(out)
        }

        pub fn values(&self) -> Result<HashMap<String, RegValue>, RegError> {
            let mut out = HashMap::new();
            let mut name = vec![0u16; 16_384];
            let mut data = vec![0u8; 4096];
            for index in 0.. {
                let (mut name_len, mut data_len, mut kind) = (name.len() as u32, data.len() as u32, 0u32);
                // SAFETY: both buffers are valid for the lengths passed, which the call updates.
                let mut err = unsafe {
                    RegEnumValueW(
                        self.0,
                        index,
                        PWSTR(name.as_mut_ptr()),
                        &mut name_len,
                        None,
                        Some(&mut kind),
                        Some(data.as_mut_ptr()),
                        Some(&mut data_len),
                    )
                };
                if err == ERROR_MORE_DATA {
                    data.resize(data_len as usize + 2, 0);
                    name_len = name.len() as u32;
                    data_len = data.len() as u32;
                    // SAFETY: as above, with the data buffer grown to the reported size.
                    err = unsafe {
                        RegEnumValueW(
                            self.0,
                            index,
                            PWSTR(name.as_mut_ptr()),
                            &mut name_len,
                            None,
                            Some(&mut kind),
                            Some(data.as_mut_ptr()),
                            Some(&mut data_len),
                        )
                    };
                }
                if err == ERROR_NO_MORE_ITEMS {
                    break;
                }
                if err != ERROR_SUCCESS {
                    return Err(map(err));
                }
                let key = String::from_utf16_lossy(&name[..name_len as usize]);
                let bytes = &data[..data_len as usize];
                let value = if kind == REG_DWORD.0 && bytes.len() >= 4 {
                    RegValue::Dword(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
                } else if kind == REG_SZ.0 || kind == REG_EXPAND_SZ.0 {
                    let units: Vec<u16> = bytes.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect();
                    RegValue::Str(String::from_utf16_lossy(&units).trim_end_matches('\0').to_string())
                } else if kind == REG_BINARY.0 {
                    RegValue::Binary(bytes.to_vec())
                } else {
                    continue;
                };
                out.insert(key, value);
            }
            Ok(out)
        }
    }

    impl Drop for Key {
        fn drop(&mut self) {
            // SAFETY: the handle came from a successful RegOpenKeyExW and is closed once.
            unsafe {
                let _ = RegCloseKey(self.0);
            }
        }
    }
}

impl Registry for WindowsRegistry {
    fn subkeys(&self, hive: Hive, view: View, path: &str) -> Result<Vec<String>, RegError> {
        win::Key::open(hive, view, path)?.subkeys()
    }

    fn values(&self, hive: Hive, view: View, path: &str) -> Result<HashMap<String, RegValue>, RegError> {
        win::Key::open(hive, view, path)?.values()
    }
}

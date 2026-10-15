//! `%ENV%` and `{KnownFolder}` expansion for rule paths. The environment is
//! injected so tests do not depend on the machine.

use std::collections::HashMap;

pub trait Env: Send + Sync {
    /// Environment variable of the interactive user (case-insensitive name).
    fn var(&self, name: &str) -> Option<String>;
    /// Known folder such as `Downloads` (from `SHGetKnownFolderPath` on Windows).
    fn known_folder(&self, name: &str) -> Option<String>;
}

/// Fixed values, for tests.
#[derive(Debug, Default, Clone)]
pub struct MapEnv {
    vars: HashMap<String, String>,
    folders: HashMap<String, String>,
}

impl MapEnv {
    pub fn with_var(mut self, k: &str, v: &str) -> Self {
        self.vars.insert(k.to_lowercase(), v.into());
        self
    }

    pub fn with_folder(mut self, k: &str, v: &str) -> Self {
        self.folders.insert(k.to_lowercase(), v.into());
        self
    }
}

impl Env for MapEnv {
    fn var(&self, name: &str) -> Option<String> {
        self.vars.get(&name.to_lowercase()).cloned()
    }

    fn known_folder(&self, name: &str) -> Option<String> {
        self.folders.get(&name.to_lowercase()).cloned()
    }
}

/// Expand a rule path to an absolute, backslash-separated path without a
/// trailing separator (except for a drive root such as `C:\`).
pub fn expand(template: &str, env: &dyn Env) -> Result<String, String> {
    let mut out = String::new();
    let mut rest = template;
    while let Some(pos) = rest.find(['%', '{']) {
        out.push_str(&rest[..pos]);
        let open = rest.as_bytes()[pos];
        let close = if open == b'%' { '%' } else { '}' };
        let after = &rest[pos + 1..];
        let Some(end) = after.find(close) else {
            return Err(format!("unterminated `{}` in `{template}`", open as char));
        };
        let name = &after[..end];
        if name.is_empty() {
            if open == b'%' {
                // `%%` is a literal percent sign.
                out.push('%');
                rest = &after[end + 1..];
                continue;
            }
            return Err(format!("empty `{{}}` in `{template}`"));
        }
        let value = if open == b'%' { env.var(name) } else { env.known_folder(name) };
        match value {
            Some(v) => out.push_str(&v),
            None => {
                return Err(if open == b'%' {
                    format!("environment variable %{name}% is not set")
                } else {
                    format!("known folder {{{name}}} is not available")
                })
            }
        }
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    normalize(&out).ok_or_else(|| format!("`{template}` does not expand to an absolute drive path"))
}

/// Normalise separators, drop `\\?\`, collapse repeats and trailing `\`.
pub fn normalize(p: &str) -> Option<String> {
    let p = p.trim().replace('/', "\\");
    let p = p.strip_prefix(r"\\?\").unwrap_or(&p);
    let mut chars = p.chars();
    let (drive, colon) = (chars.next()?, chars.next()?);
    if !drive.is_ascii_alphabetic() || colon != ':' {
        return None;
    }
    let parts: Vec<&str> = p[2..].split('\\').filter(|s| !s.is_empty()).collect();
    if parts.iter().any(|s| *s == ".." || *s == ".") {
        return None;
    }
    let mut out = format!("{}:\\", drive.to_ascii_uppercase());
    out.push_str(&parts.join("\\"));
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env() -> MapEnv {
        MapEnv::default()
            .with_var("TEMP", r"C:\Users\Ann\AppData\Local\Temp")
            .with_var("SystemDrive", "C:")
            .with_var("WINDIR", r"C:\Windows\")
            .with_folder("Downloads", r"D:\Dl")
    }

    #[test]
    fn env_vars_known_folders_and_normalisation() {
        let e = env();
        assert_eq!(expand("%TEMP%", &e).unwrap(), r"C:\Users\Ann\AppData\Local\Temp");
        assert_eq!(expand("%temp%\\x/y\\", &e).unwrap(), r"C:\Users\Ann\AppData\Local\Temp\x\y");
        assert_eq!(expand(r"%SystemDrive%\hiberfil.sys", &e).unwrap(), r"C:\hiberfil.sys");
        assert_eq!(expand(r"%WINDIR%\Temp", &e).unwrap(), r"C:\Windows\Temp");
        assert_eq!(expand("{Downloads}", &e).unwrap(), r"D:\Dl");
        assert_eq!(expand("{downloads}", &e).unwrap(), r"D:\Dl");
        assert_eq!(expand(r"c:\", &e).unwrap(), r"C:\");
    }

    #[test]
    fn unresolved_and_relative_paths_are_errors() {
        let e = env();
        assert!(expand("%NOPE%", &e).unwrap_err().contains("not set"));
        assert!(expand("{Music}", &e).unwrap_err().contains("not available"));
        assert!(expand(r"relative\dir", &e).unwrap_err().contains("absolute"));
        assert!(expand(r"C:\a\..\b", &e).is_err());
        assert!(expand("%TEMP", &e).unwrap_err().contains("unterminated"));
    }
}

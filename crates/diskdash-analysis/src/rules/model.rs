//! Rule schema. Rules are data: nothing in here can run a command or touch a file.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, TS)]
#[ts(export)]
pub enum Risk {
    Safe,
    Review,
    Expert,
}

impl Risk {
    pub const ALL: [Risk; 3] = [Risk::Safe, Risk::Review, Risk::Expert];

    pub fn as_str(self) -> &'static str {
        match self {
            Risk::Safe => "Safe",
            Risk::Review => "Review",
            Risk::Expert => "Expert",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "Safe" => Risk::Safe,
            "Review" => Risk::Review,
            "Expert" => Risk::Expert,
            _ => return None,
        })
    }

    /// Weight used to rank candidates: bytes × weight, so a Safe 2 GiB item
    /// outranks an Expert 3 GiB one.
    pub fn weight(self) -> f64 {
        match self {
            Risk::Safe => 1.0,
            Risk::Review => 0.5,
            Risk::Expert => 0.2,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
#[ts(export)]
pub enum RuleKind {
    /// Something the user can remove by hand.
    ManualCleanup,
    /// Space controlled by a Windows setting or tool; instructions only.
    SystemSetting,
}

impl RuleKind {
    pub fn as_str(self) -> &'static str {
        match self {
            RuleKind::ManualCleanup => "manual-cleanup",
            RuleKind::SystemSetting => "system-setting",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "manual-cleanup" => RuleKind::ManualCleanup,
            "system-setting" => RuleKind::SystemSetting,
            _ => return None,
        })
    }
}

/// Items that are not files in the snapshot and are measured by a probe.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
#[ts(export)]
pub enum VirtualSource {
    RecycleBin,
    ShadowStorage,
}

impl VirtualSource {
    pub fn key(self) -> &'static str {
        match self {
            VirtualSource::RecycleBin => "recycle-bin",
            VirtualSource::ShadowStorage => "shadow-storage",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum AppliesTo {
    Dir,
    File,
}

/// Categories a rule can belong to (typos are caught by validation).
pub const CATEGORIES: &[&str] =
    &["windows", "upgrade", "system", "recycle-bin", "browser", "developer", "apps", "downloads", "logs"];

/// A size such as `500000000`, `"500 MB"` or `"2 GiB"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[ts(export, type = "number")]
pub struct ByteSize(pub u64);

impl<'de> Deserialize<'de> for ByteSize {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            Num(u64),
            Text(String),
        }
        match Raw::deserialize(d)? {
            Raw::Num(n) => Ok(ByteSize(n)),
            Raw::Text(s) => parse_size(&s).map(ByteSize).ok_or_else(|| {
                serde::de::Error::custom(format!("`{s}` is not a size (examples: 500MB, 2 GiB, 1048576)"))
            }),
        }
    }
}

pub fn parse_size(s: &str) -> Option<u64> {
    let s = s.trim();
    let split = s.find(|c: char| !(c.is_ascii_digit() || c == '.')).unwrap_or(s.len());
    let (num, unit) = s.split_at(split);
    let value: f64 = num.parse().ok()?;
    let mult: f64 = match unit.trim().to_ascii_lowercase().as_str() {
        "" | "b" => 1.0,
        "kb" | "kib" | "k" => 1024.0,
        "mb" | "mib" | "m" => 1024.0 * 1024.0,
        "gb" | "gib" | "g" => 1024.0 * 1024.0 * 1024.0,
        "tb" | "tib" | "t" => 1024.0_f64.powi(4),
        _ => return None,
    };
    (value >= 0.0 && value.is_finite()).then_some((value * mult) as u64)
}

/// One name or a list of names (any of them).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(untagged)]
#[ts(export)]
pub enum OneOrMany {
    One(String),
    Many(Vec<String>),
}

impl OneOrMany {
    pub fn to_vec(&self) -> Vec<String> {
        match self {
            OneOrMany::One(s) => vec![s.clone()],
            OneOrMany::Many(v) => v.clone(),
        }
    }
}

/// The fixed condition vocabulary (no expression language on purpose).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct Conditions {
    /// The candidate's parent folder must contain one of these names (`*` allowed).
    #[serde(default)]
    #[ts(optional = nullable)]
    pub sibling_exists: Option<OneOrMany>,
    /// The candidate folder must contain one of these names (`*` allowed).
    #[serde(default)]
    #[ts(optional = nullable)]
    pub child_exists: Option<OneOrMany>,
    /// Allocated size of the candidate must be at least this.
    #[serde(default)]
    #[ts(optional = nullable)]
    pub min_size: Option<ByteSize>,
    /// Archive `X.zip` next to a folder `X` that holds at least half its size.
    #[serde(default)]
    pub extracted_sibling: bool,
}

impl Conditions {
    pub fn is_empty(&self) -> bool {
        self == &Conditions::default()
    }
}

/// A rule exactly as written in YAML. Everything is optional here so that a
/// missing field is reported with the rule's id instead of a bare serde error.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawRule {
    pub id: Option<String>,
    pub category: Option<String>,
    pub title: Option<String>,
    pub explanation: Option<String>,
    pub risk: Option<String>,
    pub kind: Option<String>,
    #[serde(default)]
    pub paths: Vec<String>,
    #[serde(default)]
    pub globs: Vec<String>,
    pub min_age_days: Option<u32>,
    #[serde(default)]
    pub applies_to: Option<AppliesTo>,
    #[serde(default)]
    pub conditions: Conditions,
    #[serde(default)]
    pub virtual_source: Option<VirtualSource>,
    pub instructions: Option<OneOrMany>,
}

/// A validated rule.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RuleDef {
    pub id: String,
    pub category: String,
    pub title: String,
    pub explanation: String,
    pub risk: Risk,
    pub kind: RuleKind,
    pub paths: Vec<String>,
    pub globs: Vec<String>,
    pub min_age_days: Option<u32>,
    pub applies_to: Option<AppliesTo>,
    pub conditions: Conditions,
    pub virtual_source: Option<VirtualSource>,
    /// Text the user follows or copies. Never executed by the app.
    pub instructions: Vec<String>,
}

/// Where a loaded rule came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "kebab-case")]
#[ts(export)]
pub enum RuleSource {
    Builtin,
    User { file: String },
}

/// A problem found while loading rules (the rule is skipped).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RuleIssue {
    pub file: String,
    pub rule_id: Option<String>,
    /// 1-based position of the rule in its file.
    pub index: Option<u32>,
    pub line: Option<u32>,
    pub column: Option<u32>,
    pub message: String,
}

impl std::fmt::Display for RuleIssue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.file)?;
        if let Some(l) = self.line {
            write!(f, ":{l}")?;
            if let Some(c) = self.column {
                write!(f, ":{c}")?;
            }
        }
        if let Some(id) = &self.rule_id {
            write!(f, " (rule {id})")?;
        } else if let Some(i) = self.index {
            write!(f, " (rule #{i})")?;
        }
        write!(f, ": {}", self.message)
    }
}

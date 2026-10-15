//! Loading the built-in rule set and the user's rule directory.

use std::collections::{BTreeSet, HashMap};
use std::path::Path;

use serde::Serialize;
use ts_rs::TS;

use super::model::{RuleDef, RuleIssue, RuleSource};
use super::parse::parse_rules;

#[derive(Debug, Clone, PartialEq)]
pub struct LoadedRule {
    pub def: RuleDef,
    pub source: RuleSource,
    /// A user rule that replaced a built-in rule with the same id.
    pub overrides_builtin: bool,
    pub disabled: bool,
}

#[derive(Debug, Default)]
pub struct RuleSet {
    pub rules: Vec<LoadedRule>,
    pub issues: Vec<RuleIssue>,
    /// Number of user rule files read.
    pub user_files: usize,
}

impl RuleSet {
    pub fn active(&self) -> impl Iterator<Item = &LoadedRule> {
        self.rules.iter().filter(|r| !r.disabled)
    }

    /// Stable hash of the active rules; changes when any rule changes.
    pub fn fingerprint(&self) -> String {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for r in self.active() {
            let text = serde_json::to_string(&r.def).unwrap_or_default();
            for b in text.bytes().chain(std::iter::once(b'\n')) {
                h ^= u64::from(b);
                h = h.wrapping_mul(0x0000_0100_0000_01b3);
            }
        }
        format!("{h:016x}")
    }
}

fn issue(file: &str, rule_id: Option<&str>, message: String) -> RuleIssue {
    RuleIssue {
        file: file.into(),
        rule_id: rule_id.map(str::to_string),
        index: None,
        line: None,
        column: None,
        message,
    }
}

/// Load `builtin` (file name, YAML) and every `*.yaml`/`*.yml` in `user_dir`.
/// A user rule with a built-in's id replaces it; ids under `disabled:` are
/// kept in the list but flagged and never evaluated.
pub fn load(builtin: &[(&str, &str)], user_dir: Option<&Path>) -> RuleSet {
    let mut set = RuleSet::default();
    let mut index: HashMap<String, usize> = HashMap::new();
    let mut disabled: BTreeSet<String> = BTreeSet::new();

    for (name, src) in builtin {
        let parsed = parse_rules(name, src);
        set.issues.extend(parsed.issues);
        disabled.extend(parsed.disabled);
        for def in parsed.rules {
            if index.contains_key(&def.id) {
                set.issues.push(issue(name, Some(&def.id), "duplicate built-in rule id".into()));
                continue;
            }
            index.insert(def.id.clone(), set.rules.len());
            set.rules.push(LoadedRule { def, source: RuleSource::Builtin, overrides_builtin: false, disabled: false });
        }
    }

    let builtin_count = set.rules.len();
    let mut user_seen: HashMap<String, String> = HashMap::new();
    if let Some(dir) = user_dir {
        for (name, result) in read_user_files(dir) {
            set.user_files += 1;
            let src = match result {
                Ok(s) => s,
                Err(e) => {
                    set.issues.push(issue(&name, None, format!("cannot read the file: {e}")));
                    continue;
                }
            };
            let parsed = parse_rules(&name, &src);
            set.issues.extend(parsed.issues);
            disabled.extend(parsed.disabled);
            for def in parsed.rules {
                if let Some(first) = user_seen.get(&def.id) {
                    set.issues.push(issue(
                        &name,
                        Some(&def.id),
                        format!("duplicate rule id (already defined in {first})"),
                    ));
                    continue;
                }
                user_seen.insert(def.id.clone(), name.clone());
                let source = RuleSource::User { file: name.clone() };
                match index.get(&def.id) {
                    Some(&i) if i < builtin_count => {
                        set.rules[i] = LoadedRule { def, source, overrides_builtin: true, disabled: false };
                    }
                    _ => {
                        index.insert(def.id.clone(), set.rules.len());
                        set.rules.push(LoadedRule { def, source, overrides_builtin: false, disabled: false });
                    }
                }
            }
        }
    }
    for r in &mut set.rules {
        r.disabled = disabled.contains(&r.def.id);
    }
    for id in &disabled {
        if !index.contains_key(id) {
            tracing::warn!(id, "`disabled` names a rule that does not exist");
        }
    }
    for i in &set.issues {
        tracing::warn!(issue = %i, "cleanup rule skipped");
    }
    set
}

fn read_user_files(dir: &Path) -> Vec<(String, std::io::Result<String>)> {
    let Ok(rd) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut files: Vec<_> = rd
        .flatten()
        .filter(|e| {
            let n = e.file_name().to_string_lossy().to_lowercase();
            (n.ends_with(".yaml") || n.ends_with(".yml")) && e.file_type().is_ok_and(|t| t.is_file())
        })
        .map(|e| e.path())
        .collect();
    files.sort();
    files
        .into_iter()
        .map(|p| {
            let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            (name, std::fs::read_to_string(&p))
        })
        .collect()
}

/// For the Rules page: where user rules go.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct UserRulesDir {
    pub path: String,
    pub exists: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(id: &str, title: &str) -> String {
        format!(
            "  - id: {id}\n    category: windows\n    title: {title}\n    explanation: e\n    risk: Safe\n    kind: manual-cleanup\n    paths: [\"C:\\\\x\"]\n    instructions: do it\n"
        )
    }

    #[test]
    fn user_rule_overrides_builtin_and_broken_file_is_isolated() {
        let tmp = tempfile::tempdir().unwrap();
        test_fixtures::write_text(&tmp.path().join("a.yaml"), &format!("rules:\n{}", rule("one", "Mine")));
        test_fixtures::write_text(&tmp.path().join("broken.yaml"), "rules:\n  - id: x\n   bad: [\n");
        test_fixtures::write_text(&tmp.path().join("notes.txt"), "ignored");
        let builtin = format!("rules:\n{}{}", rule("one", "Built-in"), rule("two", "Two"));
        let set = load(&[("windows.yaml", &builtin)], Some(tmp.path()));

        assert_eq!(set.rules.len(), 2);
        let one = set.rules.iter().find(|r| r.def.id == "one").unwrap();
        assert_eq!(one.def.title, "Mine");
        assert!(one.overrides_builtin);
        assert!(matches!(one.source, RuleSource::User { .. }));
        assert_eq!(set.rules.iter().find(|r| r.def.id == "two").unwrap().def.title, "Two");
        assert_eq!(set.user_files, 2);
        assert_eq!(set.issues.len(), 1);
        assert_eq!(set.issues[0].file, "broken.yaml");
        assert!(set.issues[0].line.is_some());
    }

    #[test]
    fn disabled_ids_are_flagged_not_evaluated() {
        let tmp = tempfile::tempdir().unwrap();
        test_fixtures::write_text(&tmp.path().join("off.yaml"), "disabled: [two]\n");
        let builtin = format!("rules:\n{}{}", rule("one", "One"), rule("two", "Two"));
        let set = load(&[("w.yaml", &builtin)], Some(tmp.path()));
        assert_eq!(set.rules.len(), 2);
        assert_eq!(set.active().count(), 1);
        assert!(set.rules.iter().find(|r| r.def.id == "two").unwrap().disabled);
    }

    #[test]
    fn missing_user_dir_is_fine_and_fingerprint_tracks_changes() {
        let a = load(&[("w.yaml", &format!("rules:\n{}", rule("one", "One")))], Some(Path::new(r"C:\no\such\dir")));
        let b = load(&[("w.yaml", &format!("rules:\n{}", rule("one", "Changed")))], None);
        assert!(a.issues.is_empty());
        assert_ne!(a.fingerprint(), b.fingerprint());
        assert_eq!(a.fingerprint(), a.fingerprint());
    }
}

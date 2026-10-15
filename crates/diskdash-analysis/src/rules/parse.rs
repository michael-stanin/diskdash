//! YAML → validated rules. The only module that knows which YAML crate is used,
//! so it can be swapped (`serde_yaml` itself is archived).
//!
//! Each rule is deserialised on its own: one broken rule never takes the rest
//! of its file down.

use serde_yaml_ng::Value;

use super::model::{RawRule, Risk, RuleDef, RuleIssue, RuleKind, CATEGORIES};

#[derive(Debug, Default)]
pub struct ParsedFile {
    pub rules: Vec<RuleDef>,
    pub disabled: Vec<String>,
    pub issues: Vec<RuleIssue>,
}

fn issue(file: &str, message: impl Into<String>) -> RuleIssue {
    RuleIssue { file: file.into(), rule_id: None, index: None, line: None, column: None, message: message.into() }
}

/// 1-based line of `id: <id>` in the source, when it can be found.
fn locate(source: &str, id: &str) -> Option<u32> {
    source
        .lines()
        .position(|l| {
            let t = l.trim_start().trim_start_matches("- ").trim_start();
            t.strip_prefix("id:").is_some_and(|v| v.trim().trim_matches(['"', '\'']) == id)
        })
        .map(|i| i as u32 + 1)
}

/// Parse one rule file. `file` is only used in messages.
pub fn parse_rules(file: &str, source: &str) -> ParsedFile {
    let mut out = ParsedFile::default();
    let doc: Value = match serde_yaml_ng::from_str(source) {
        Ok(v) => v,
        Err(e) => {
            let mut i = issue(file, format!("invalid YAML: {}", e.to_string().split(" at line").next().unwrap_or("")));
            if let Some(loc) = e.location() {
                i.line = Some(loc.line() as u32);
                i.column = Some(loc.column() as u32);
            }
            out.issues.push(i);
            return out;
        }
    };
    let map = match doc {
        Value::Null => return out,
        Value::Mapping(m) => m,
        _ => {
            out.issues.push(issue(file, "the file must contain `rules:` (and optionally `disabled:`)"));
            return out;
        }
    };
    for (k, v) in &map {
        match k.as_str() {
            Some("rules") => match v {
                Value::Sequence(items) => {
                    for (n, item) in items.iter().enumerate() {
                        match parse_rule(file, source, n as u32 + 1, item) {
                            Ok(rule) => out.rules.push(rule),
                            Err(i) => out.issues.push(i),
                        }
                    }
                }
                Value::Null => {}
                _ => out.issues.push(issue(file, "`rules` must be a list")),
            },
            Some("disabled") => match serde_yaml_ng::from_value::<Vec<String>>(v.clone()) {
                Ok(ids) => out.disabled.extend(ids),
                Err(_) => out.issues.push(issue(file, "`disabled` must be a list of rule ids")),
            },
            Some(other) => out.issues.push(issue(file, format!("unknown top-level key `{other}`"))),
            None => out.issues.push(issue(file, "unexpected top-level key")),
        }
    }
    out
}

fn parse_rule(file: &str, source: &str, index: u32, item: &Value) -> Result<RuleDef, RuleIssue> {
    let id_hint = item.get("id").and_then(Value::as_str).map(str::to_string);
    let fail = |msg: String| RuleIssue {
        file: file.into(),
        rule_id: id_hint.clone(),
        index: Some(index),
        line: id_hint.as_deref().and_then(|id| locate(source, id)),
        column: None,
        message: msg,
    };
    let raw: RawRule = serde_yaml_ng::from_value(item.clone()).map_err(|e| fail(e.to_string()))?;
    validate(raw).map_err(fail)
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 80
        && id.chars().next().is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        && id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '-' | '_' | '.'))
}

fn required(v: Option<String>, name: &str) -> Result<String, String> {
    match v.map(|s| s.trim().to_string()) {
        Some(s) if !s.is_empty() => Ok(s),
        _ => Err(format!("`{name}` is required")),
    }
}

/// Semantic validation of one rule.
pub fn validate(raw: RawRule) -> Result<RuleDef, String> {
    let id = required(raw.id, "id")?;
    if !valid_id(&id) {
        return Err(format!("`id` must be lowercase letters, digits, `-`, `_` or `.` (got `{id}`)"));
    }
    let category = required(raw.category, "category")?;
    if !CATEGORIES.contains(&category.as_str()) {
        return Err(format!("unknown `category` `{category}` (use one of: {})", CATEGORIES.join(", ")));
    }
    let title = required(raw.title, "title")?;
    let explanation = required(raw.explanation, "explanation")?;
    let risk_text = required(raw.risk, "risk")?;
    let risk =
        Risk::parse(&risk_text).ok_or_else(|| format!("`risk` must be Safe, Review or Expert (got `{risk_text}`)"))?;
    let kind_text = required(raw.kind, "kind")?;
    let kind = RuleKind::parse(&kind_text)
        .ok_or_else(|| format!("`kind` must be manual-cleanup or system-setting (got `{kind_text}`)"))?;

    let instructions: Vec<String> = raw
        .instructions
        .map(|i| i.to_vec())
        .unwrap_or_default()
        .into_iter()
        .map(|s| s.trim_end().to_string())
        .filter(|s| !s.trim().is_empty())
        .collect();
    if instructions.is_empty() {
        return Err("`instructions` is required: say how the user can do this by hand".into());
    }

    if raw.paths.is_empty() && raw.globs.is_empty() && raw.virtual_source.is_none() {
        return Err("a rule needs `paths`, `globs` or `virtual_source`".into());
    }
    if raw.virtual_source.is_some() && (!raw.paths.is_empty() || !raw.globs.is_empty()) {
        return Err("`virtual_source` cannot be combined with `paths` or `globs`".into());
    }
    for g in &raw.globs {
        if g.trim().is_empty() {
            return Err("empty glob".into());
        }
        if let Err(e) = globset::Glob::new(g) {
            return Err(format!("invalid glob `{g}`: {}", e.kind()));
        }
    }
    if raw.paths.iter().any(|p| p.trim().is_empty()) {
        return Err("empty path".into());
    }
    if raw.conditions.extracted_sibling && raw.applies_to == Some(super::model::AppliesTo::Dir) {
        return Err("`extracted_sibling` applies to archive files, not folders".into());
    }

    Ok(RuleDef {
        id,
        category,
        title,
        explanation,
        risk,
        kind,
        paths: raw.paths.into_iter().map(|p| p.trim().to_string()).collect(),
        globs: raw.globs.into_iter().map(|g| g.trim().to_string()).collect(),
        min_age_days: raw.min_age_days,
        applies_to: raw.applies_to,
        conditions: raw.conditions,
        virtual_source: raw.virtual_source,
        instructions,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = r#"
rules:
  - id: temp
    category: windows
    title: Temp
    explanation: Scratch files.
    risk: Safe
    kind: manual-cleanup
    paths: ["%TEMP%"]
    instructions: Open Settings > Storage > Temporary files.
"#;

    #[test]
    fn valid_rule_loads() {
        let p = parse_rules("a.yaml", GOOD);
        assert!(p.issues.is_empty(), "{:?}", p.issues);
        assert_eq!(p.rules.len(), 1);
        assert_eq!(p.rules[0].instructions.len(), 1);
        assert_eq!(p.rules[0].risk, Risk::Safe);
    }

    #[test]
    fn missing_instructions_names_file_and_rule() {
        let src = GOOD.replace("    instructions: Open Settings > Storage > Temporary files.\n", "");
        let p = parse_rules("user.yaml", &src);
        assert!(p.rules.is_empty());
        let i = &p.issues[0];
        assert_eq!(i.file, "user.yaml");
        assert_eq!(i.rule_id.as_deref(), Some("temp"));
        assert!(i.message.contains("instructions"));
        assert_eq!(i.line, Some(3), "line of `id:`");
    }

    #[test]
    fn one_bad_rule_does_not_stop_the_others() {
        let src = format!("{GOOD}  - id: bad\n    risk: Nope\n");
        let p = parse_rules("a.yaml", &src);
        assert_eq!(p.rules.len(), 1);
        assert_eq!(p.issues.len(), 1);
        assert_eq!(p.issues[0].rule_id.as_deref(), Some("bad"));
    }

    #[test]
    fn invalid_yaml_reports_line() {
        let p = parse_rules("broken.yaml", "rules:\n  - id: x\n   bad: [\n");
        assert!(p.rules.is_empty());
        assert!(p.issues[0].line.is_some(), "{:?}", p.issues);
        assert!(p.issues[0].message.starts_with("invalid YAML"));
    }

    #[test]
    fn unknown_fields_and_values_are_rejected() {
        let p = parse_rules("a.yaml", &GOOD.replace("risk: Safe", "risk: Safe\n    colour: red"));
        assert!(p.issues[0].message.contains("colour"), "{:?}", p.issues);
        let p = parse_rules("a.yaml", &GOOD.replace("category: windows", "category: windwos"));
        assert!(p.issues[0].message.contains("unknown `category`"));
        let p = parse_rules("a.yaml", &GOOD.replace("paths: [\"%TEMP%\"]", "globs: [\"[\"]"));
        assert!(p.issues[0].message.contains("invalid glob"));
    }

    #[test]
    fn disabled_list_and_size_strings() {
        let src = format!("disabled: [a, b]\n{GOOD}",).replace(
            "kind: manual-cleanup",
            "kind: manual-cleanup\n    conditions:\n      min_size: 1.5 GiB\n      sibling_exists: [a.txt, b.txt]",
        );
        let p = parse_rules("a.yaml", &src);
        assert!(p.issues.is_empty(), "{:?}", p.issues);
        assert_eq!(p.disabled, ["a", "b"]);
        assert_eq!(p.rules[0].conditions.min_size.unwrap().0, (1.5 * 1024.0 * 1024.0 * 1024.0) as u64);
    }

    #[test]
    fn empty_file_is_fine() {
        assert!(parse_rules("e.yaml", "").issues.is_empty());
        assert!(parse_rules("e.yaml", "# only a comment\n").issues.is_empty());
    }
}

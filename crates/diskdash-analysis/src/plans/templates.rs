//! Instruction templates (YAML, embedded from `rules/plan-instructions.yaml`).

use std::collections::BTreeMap;

use serde::Deserialize;

const BUILTIN: &str = include_str!("../../../../rules/plan-instructions.yaml");

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    templates: BTreeMap<String, Vec<String>>,
}

/// Values substituted into templates.
#[derive(Debug, Default, Clone)]
pub struct TemplateCtx {
    pub path: String,
    pub destination: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Templates {
    map: BTreeMap<String, Vec<String>>,
}

/// Quotes would break the quoting of a command line shown to the user.
fn clean(s: &str) -> String {
    s.replace('"', "")
}

impl Templates {
    pub fn builtin() -> Self {
        Self::parse(BUILTIN).unwrap_or_else(|e| {
            tracing::error!(error = %e, "built-in plan instructions are invalid");
            Self { map: BTreeMap::new() }
        })
    }

    pub fn parse(yaml: &str) -> Result<Self, String> {
        let f: File = serde_yaml_ng::from_str(yaml).map_err(|e| e.to_string())?;
        if let Some((k, _)) = f.templates.iter().find(|(_, v)| v.is_empty()) {
            return Err(format!("template `{k}` has no lines"));
        }
        Ok(Self { map: f.templates })
    }

    pub fn keys(&self) -> impl Iterator<Item = &String> {
        self.map.keys()
    }

    /// The instruction lines for `key`, with placeholders filled in.
    pub fn render(&self, key: &str, ctx: &TemplateCtx) -> Vec<String> {
        let path = clean(&ctx.path);
        let (dir, name) = match path.rsplit_once('\\') {
            Some((d, n)) => (d.to_string(), n.to_string()),
            None => (String::new(), path.clone()),
        };
        let destination = clean(ctx.destination.as_deref().unwrap_or("<destination>"));
        self.map
            .get(key)
            .map(|lines| {
                lines
                    .iter()
                    .map(|l| {
                        l.replace("{path}", &path)
                            .replace("{dir}", &dir)
                            .replace("{name}", &name)
                            .replace("{destination}", &destination)
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_templates_load_and_cover_every_kind() {
        let t = Templates::builtin();
        for k in ["explorer-delete", "duplicate-remove", "stale-review", "manual-move-dir", "manual-move-file"] {
            assert!(!t.render(k, &TemplateCtx::default()).is_empty(), "{k}");
        }
    }

    #[test]
    fn move_templates_quote_paths_and_substitute() {
        let t = Templates::builtin();
        let ctx = TemplateCtx { path: r"D:\Games\My Game".into(), destination: Some(r"E:\Games\My Game".into()) };
        let lines = t.render("manual-move-dir", &ctx);
        assert!(
            lines.iter().any(|l| l == r#"robocopy "D:\Games\My Game" "E:\Games\My Game" /E /COPY:DAT /DCOPY:T"#),
            "{lines:?}"
        );
        let file = t.render(
            "manual-move-file",
            &TemplateCtx { path: r"D:\v\movie.mkv".into(), destination: Some(r"E:\v".into()) },
        );
        assert!(file.iter().any(|l| l == r#"robocopy "D:\v" "E:\v" "movie.mkv" /COPY:DAT"#), "{file:?}");
    }

    #[test]
    fn quotes_in_paths_cannot_break_the_command() {
        let t = Templates::builtin();
        let ctx = TemplateCtx { path: "D:\\a\"; del *\\x".into(), destination: Some("E:\\\"b".into()) };
        let joined = t.render("manual-move-dir", &ctx).join("\n");
        assert!(!joined.contains("\"; del"));
        assert!(joined.contains(r#"robocopy "D:\a; del *\x" "E:\b""#), "{joined}");
    }

    #[test]
    fn invalid_templates_are_rejected() {
        assert!(Templates::parse("templates:\n  a: []\n").is_err());
        assert!(Templates::parse("nonsense: 1").is_err());
    }
}

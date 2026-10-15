//! Plan export: CSV and TXT with paths and instructions only. Never a script:
//! no executable extension, no shell header, nothing that could be run directly.

#[derive(Debug, Clone)]
pub struct ExportItem {
    pub status: String,
    pub risk: String,
    pub kind: String,
    pub path: String,
    pub logical: Option<u64>,
    pub allocated: Option<u64>,
    pub instructions: Vec<String>,
    pub note: Option<String>,
}

fn drive_of(path: &str) -> String {
    path.get(..2).filter(|d| d.ends_with(':')).map_or_else(|| "Other".to_string(), str::to_uppercase)
}

/// RFC 4180 quoting. Free text that a spreadsheet could read as a formula gets a leading apostrophe.
fn field(s: &str, guard_formula: bool) -> String {
    let s =
        if guard_formula && s.starts_with(['=', '+', '-', '@', '\t', '\r']) { format!("'{s}") } else { s.to_string() };
    if s.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s
    }
}

pub fn to_csv(items: &[ExportItem]) -> String {
    let mut out = String::from("status,risk,kind,path,logical_bytes,allocated_bytes,instructions,note\r\n");
    for i in items {
        let num = |v: Option<u64>| v.map_or(String::new(), |v| v.to_string());
        let row = [
            field(&i.status, false),
            field(&i.risk, false),
            field(&i.kind, false),
            field(&i.path, false),
            num(i.logical),
            num(i.allocated),
            field(&i.instructions.join(" | "), true),
            field(i.note.as_deref().unwrap_or(""), true),
        ];
        out.push_str(&row.join(","));
        out.push_str("\r\n");
    }
    out
}

fn size(v: Option<u64>) -> String {
    match v {
        None => "size unknown".into(),
        Some(b) => {
            let units = ["B", "KiB", "MiB", "GiB", "TiB"];
            let (mut x, mut u) = (b as f64, 0);
            while x >= 1024.0 && u < units.len() - 1 {
                x /= 1024.0;
                u += 1;
            }
            if u == 0 {
                format!("{b} B")
            } else {
                format!("{x:.2} {}", units[u])
            }
        }
    }
}

/// A human-readable checklist grouped by drive and kind.
pub fn to_txt(plan_name: &str, items: &[ExportItem]) -> String {
    let mut out = format!("Cleanup plan: {plan_name}\r\nPaths and steps for you to carry out by hand. diskdash did not change anything.\r\n");
    let total: u64 = items.iter().filter_map(|i| i.allocated).sum();
    out.push_str(&format!("{} item(s), {} on disk\r\n", items.len(), size(Some(total))));
    let mut groups: std::collections::BTreeMap<(String, String), Vec<&ExportItem>> = Default::default();
    for i in items {
        groups.entry((drive_of(&i.path), i.kind.clone())).or_default().push(i);
    }
    for ((drive, kind), list) in groups {
        out.push_str(&format!("\r\n== {drive}  {kind} ==\r\n"));
        for i in list {
            let mark = match i.status.as_str() {
                "done" => "[x]",
                "skipped" => "[-]",
                _ => "[ ]",
            };
            out.push_str(&format!("{mark} {}  ({}, {} risk)\r\n", i.path, size(i.allocated), i.risk));
            for l in &i.instructions {
                out.push_str(&format!("      {l}\r\n"));
            }
            if let Some(n) = i.note.as_deref().filter(|n| !n.is_empty()) {
                out.push_str(&format!("      Note: {n}\r\n"));
            }
        }
    }
    out
}

/// A safe download name; the extension is always `csv` or `txt`.
pub fn file_name(plan_name: &str, csv: bool) -> String {
    let slug: String = plan_name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    let slug = if slug.is_empty() { "plan".to_string() } else { slug.chars().take(40).collect() };
    format!("diskdash-plan-{slug}.{}", if csv { "csv" } else { "txt" })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(path: &str, note: Option<&str>) -> ExportItem {
        ExportItem {
            status: "todo".into(),
            risk: "Safe".into(),
            kind: "manual-cleanup".into(),
            path: path.into(),
            logical: Some(1000),
            allocated: Some(4096),
            instructions: vec!["Delete it in Explorer.".into(), "powercfg /h off".into()],
            note: note.map(String::from),
        }
    }

    #[test]
    fn csv_escapes_commas_quotes_and_newlines() {
        let csv = to_csv(&[item(r#"D:\a, "b"\c.txt"#, Some("line1\nline2"))]);
        let lines: Vec<&str> = csv.split("\r\n").collect();
        assert_eq!(lines[0], "status,risk,kind,path,logical_bytes,allocated_bytes,instructions,note");
        assert!(lines[1].contains(r#""D:\a, ""b""\c.txt""#), "{}", lines[1]);
        assert!(lines[1].contains("\"line1\nline2\"") || csv.contains("\"line1\nline2\""));
        assert!(lines[1].contains("1000,4096"));
    }

    #[test]
    fn spreadsheet_formulas_in_free_text_are_defused() {
        let csv = to_csv(&[item(r"D:\a", Some("=HYPERLINK(\"http://x\")"))]);
        assert!(csv.contains("'=HYPERLINK"), "{csv}");
    }

    #[test]
    fn txt_is_a_checklist_without_script_content() {
        let mut done = item(r"C:\x", None);
        done.status = "done".into();
        let txt = to_txt("My plan", &[done, item(r"D:\y", Some("keep an eye"))]);
        assert!(txt.starts_with("Cleanup plan: My plan"));
        assert!(txt.contains("[x] C:\\x"));
        assert!(txt.contains("[ ] D:\\y"));
        assert!(txt.contains("== C:  manual-cleanup =="));
        assert!(txt.contains("Note: keep an eye"));
        for bad in ["#!", "@echo", "powershell -", "Remove-Item"] {
            assert!(!txt.contains(bad), "{bad}");
        }
    }

    #[test]
    fn file_names_never_have_script_extensions() {
        assert_eq!(file_name("My plan: C:\\temp!", true), "diskdash-plan-my-plan-c-temp.csv");
        assert_eq!(file_name("../../evil.bat", false), "diskdash-plan-evil-bat.txt");
        assert_eq!(file_name("", true), "diskdash-plan-plan.csv");
        for n in ["a.ps1", "x.cmd", "run.bat"] {
            let name = file_name(n, true);
            assert!(
                name.ends_with(".csv") && !name.contains(".ps1") && !name.contains(".bat") && !name.contains(".cmd"),
                "{name}"
            );
        }
    }
}

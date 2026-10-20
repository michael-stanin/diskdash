//! CSV export: one file per section, bundled in a ZIP. Sizes are integer bytes,
//! timestamps UTC RFC 3339. Text cells that a spreadsheet could read as a formula
//! (`=`, `+`, `-`, `@`) get a leading apostrophe.

use std::io::{Cursor, Write};

use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

use super::model::*;

/// Neutralise a text cell that could be evaluated as a formula when opened in a spreadsheet.
pub fn text(s: &str) -> String {
    match s.chars().next() {
        Some('=' | '+' | '-' | '@' | '\t' | '\r') => format!("'{s}"),
        _ => s.to_string(),
    }
}

fn num<T: ToString>(n: T) -> String {
    n.to_string()
}

fn opt<T: ToString>(n: Option<T>) -> String {
    n.map(|v| v.to_string()).unwrap_or_default()
}

fn float(n: Option<f64>) -> String {
    n.map(|v| format!("{v:.2}")).unwrap_or_default()
}

struct Table {
    w: csv::Writer<Vec<u8>>,
}

impl Table {
    fn new(header: &[&str]) -> Self {
        let mut w = csv::WriterBuilder::new().terminator(csv::Terminator::CRLF).from_writer(Vec::new());
        let _ = w.write_record(header);
        Self { w }
    }

    fn row(&mut self, cells: Vec<String>) {
        let _ = self.w.write_record(&cells);
    }

    fn finish(self) -> Vec<u8> {
        let mut out = b"\xEF\xBB\xBF".to_vec(); // a byte-order mark so Excel reads UTF-8 paths correctly
        out.extend(self.w.into_inner().unwrap_or_default());
        out
    }
}

/// The CSV files of a report as `(file name, content)`.
pub fn files(r: &Report) -> Vec<(String, Vec<u8>)> {
    let mut out: Vec<(String, Vec<u8>)> = Vec::new();
    let s = &r.sections;

    if let Some(d) = s.drives.as_ref().and_then(|x| x.data.as_ref()) {
        let mut t = Table::new(&[
            "drive",
            "label",
            "filesystem",
            "media",
            "capacity_bytes",
            "free_bytes",
            "used_bytes",
            "used_percent",
            "health",
            "low_space",
        ]);
        for x in d {
            t.row(vec![
                text(&x.drive),
                text(&x.label),
                text(&x.filesystem),
                text(&x.media),
                num(x.capacity),
                num(x.free),
                num(x.used),
                num(x.used_percent),
                text(&x.health),
                num(x.low_space),
            ]);
        }
        out.push(("drives.csv".into(), t.finish()));
    }
    if let Some(top) = s.top.as_ref().and_then(|x| x.data.as_ref()) {
        for (name, pick) in [("top-folders.csv", true), ("top-files.csv", false)] {
            let mut t =
                Table::new(&["root", "snapshot_id", "scanned_at", "rank", "path", "allocated_bytes", "logical_bytes"]);
            for root in top {
                let list = if pick { &root.folders } else { &root.files };
                for (i, e) in list.iter().enumerate() {
                    t.row(vec![
                        text(&root.root),
                        num(root.snapshot_id),
                        opt(root.scanned_at.as_ref()),
                        num(i + 1),
                        text(&e.path),
                        num(e.allocated),
                        num(e.logical),
                    ]);
                }
            }
            out.push((name.into(), t.finish()));
        }
    }
    if let Some(c) = s.cleanup.as_ref().and_then(|x| x.data.as_ref()) {
        let mut t = Table::new(&["root", "risk", "kind", "allocated_bytes", "logical_bytes", "matches"]);
        for x in &c.savings {
            t.row(vec![text(&x.root), text(&x.risk), text(&x.kind), num(x.allocated), num(x.logical), num(x.matches)]);
        }
        out.push(("cleanup-savings.csv".into(), t.finish()));
        let mut t = Table::new(&[
            "root",
            "rule_id",
            "title",
            "risk",
            "kind",
            "path",
            "allocated_bytes",
            "logical_bytes",
            "instructions",
        ]);
        for x in &c.candidates {
            t.row(vec![
                text(&x.root),
                text(&x.rule_id),
                text(&x.title),
                text(&x.risk),
                text(&x.kind),
                text(&x.path),
                opt(x.allocated),
                opt(x.logical),
                text(&x.instructions.join("\n")),
            ]);
        }
        out.push(("cleanup-candidates.csv".into(), t.finish()));
    }
    if let Some(d) = s.duplicates.as_ref().and_then(|x| x.data.as_ref()) {
        let mut t = Table::new(&["group", "size_bytes", "wasted_bytes", "path", "suggested"]);
        for (i, g) in d.groups.iter().enumerate() {
            for c in &g.copies {
                t.row(vec![
                    num(i + 1),
                    num(g.size),
                    num(g.wasted),
                    text(&c.path),
                    text(c.suggested.as_deref().unwrap_or("")),
                ]);
            }
        }
        out.push(("duplicates.csv".into(), t.finish()));
    }
    if let Some(st) = s.stale.as_ref().and_then(|x| x.data.as_ref()) {
        let mut t = Table::new(&["root", "months", "time_basis", "path", "allocated_bytes", "last_used", "age_days"]);
        for r in &st.roots {
            for i in &r.items {
                t.row(vec![
                    text(&r.root),
                    num(r.months),
                    text(&r.time_basis),
                    text(&i.path),
                    num(i.allocated),
                    i.last_used.clone(),
                    num(i.age_days),
                ]);
            }
        }
        out.push(("stale.csv".into(), t.finish()));
    }
    if let Some(ti) = s.tiering.as_ref().and_then(|x| x.data.as_ref()) {
        let mut t = Table::new(&[
            "path",
            "direction",
            "kind",
            "allocated_bytes",
            "from_drive",
            "to_drive",
            "destination",
            "reason",
            "instructions",
        ]);
        for x in &ti.suggestions {
            t.row(vec![
                text(&x.path),
                text(&x.direction),
                text(&x.kind),
                num(x.allocated),
                text(&x.from_drive),
                text(x.to_drive.as_deref().unwrap_or("")),
                text(x.destination.as_deref().unwrap_or("")),
                text(&x.reason),
                text(&x.instructions.join("\n")),
            ]);
        }
        out.push(("tiering.csv".into(), t.finish()));
    }
    if let Some(p) = s.plans.as_ref().and_then(|x| x.data.as_ref()) {
        let mut t = Table::new(&[
            "plan",
            "created_at",
            "path",
            "title",
            "risk",
            "allocated_bytes",
            "status",
            "destination",
            "instructions",
        ]);
        for plan in &p.plans {
            for i in &plan.items {
                t.row(vec![
                    text(&plan.name),
                    plan.created_at.clone(),
                    text(&i.path),
                    text(&i.title),
                    text(&i.risk),
                    opt(i.allocated),
                    text(&i.status),
                    text(i.destination.as_deref().unwrap_or("")),
                    text(&i.instructions.join("\n")),
                ]);
            }
        }
        out.push(("plans.csv".into(), t.finish()));
    }
    if let Some(f) = s.forecast.as_ref().and_then(|x| x.data.as_ref()) {
        let mut t = Table::new(&[
            "drive",
            "capacity_bytes",
            "state",
            "days_to_low",
            "days_to_full",
            "bytes_per_day",
            "confidence",
        ]);
        let mut samples = Table::new(&["drive", "timestamp", "free_bytes"]);
        for v in &f.volumes {
            t.row(vec![
                text(&v.drive),
                num(v.capacity),
                text(&v.state),
                float(v.days_to_low),
                float(v.days_to_full),
                float(v.bytes_per_day),
                text(&v.confidence),
            ]);
            for [ts, free] in &v.samples {
                samples.row(vec![
                    text(&v.drive),
                    humantime::format_rfc3339_seconds(
                        std::time::UNIX_EPOCH + std::time::Duration::from_secs((*ts).max(0) as u64),
                    )
                    .to_string(),
                    num(free),
                ]);
            }
        }
        out.push(("forecast.csv".into(), t.finish()));
        out.push(("forecast-samples.csv".into(), samples.finish()));
    }
    out
}

fn status_of<T>(s: &Option<Section<T>>) -> Option<serde_json::Value> {
    s.as_ref().map(|s| serde_json::json!({ "status": s.status, "reason": s.reason, "truncated": s.truncated, "total_rows": s.total_rows }))
}

/// `manifest.json`: which sections are present, which were not available, and why.
pub fn manifest(r: &Report, file_names: &[String]) -> Vec<u8> {
    let s = &r.sections;
    let mut sections = serde_json::Map::new();
    for (k, v) in [
        ("drives", status_of(&s.drives)),
        ("top", status_of(&s.top)),
        ("cleanup", status_of(&s.cleanup)),
        ("duplicates", status_of(&s.duplicates)),
        ("stale", status_of(&s.stale)),
        ("tiering", status_of(&s.tiering)),
        ("plans", status_of(&s.plans)),
        ("forecast", status_of(&s.forecast)),
    ] {
        if let Some(v) = v {
            sections.insert(k.into(), v);
        }
    }
    serde_json::to_vec_pretty(&serde_json::json!({
        "schema_version": r.schema_version,
        "generated_at": r.generated_at,
        "app_version": r.app_version,
        "scope": r.scope,
        "files": file_names,
        "sections": sections,
        "notes": "Sizes are integer bytes. Timestamps are UTC (RFC 3339). Instructions are text for a person to read; diskdash never runs them.",
    }))
    .unwrap_or_default()
}

/// The ZIP of all CSV files plus the manifest.
pub fn to_zip(r: &Report) -> Result<Vec<u8>, String> {
    let mut csvs = files(r);
    let names: Vec<String> = csvs.iter().map(|(n, _)| n.clone()).collect();
    csvs.push(("manifest.json".into(), manifest(r, &names)));
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    let opts = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    for (name, bytes) in &csvs {
        zip.start_file(name.as_str(), opts).map_err(|e| e.to_string())?;
        zip.write_all(bytes).map_err(|e| e.to_string())?;
    }
    Ok(zip.finish().map_err(|e| e.to_string())?.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formula_looking_text_is_neutralised() {
        assert_eq!(text("=HYPERLINK(\"x\")"), "'=HYPERLINK(\"x\")");
        assert_eq!(text("+1"), "'+1");
        assert_eq!(text("-2"), "'-2");
        assert_eq!(text("@SUM(A1)"), "'@SUM(A1)");
        assert_eq!(text("C:\\Users"), "C:\\Users");
        assert_eq!(text(""), "");
    }
}

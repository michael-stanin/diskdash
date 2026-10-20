//! The self-contained HTML report: askama template compiled into the binary,
//! inline CSS, inline SVG charts. Nothing external, no script.

use askama::Template;

use super::charts::{self, bytes_label, palette};
use super::model::*;

pub struct Head {
    pub text: String,
    pub num: bool,
}

pub struct Cell {
    pub text: String,
    pub num: bool,
    pub path: bool,
    pub pre: bool,
}

pub struct TableView {
    pub headers: Vec<Head>,
    pub rows: Vec<Vec<Cell>>,
}

pub enum Block {
    Heading(String),
    Note(String),
    Svg(String),
    Table(TableView),
}

pub struct SectionView {
    pub id: &'static str,
    pub title: &'static str,
    pub blocks: Vec<Block>,
}

#[derive(Template)]
#[template(path = "report.html")]
struct Page<'a> {
    version: &'a str,
    scope: String,
    generated_at: &'a str,
    sections: Vec<SectionView>,
}

fn h(text: &str, num: bool) -> Head {
    Head { text: text.into(), num }
}

fn t(text: impl Into<String>) -> Cell {
    Cell { text: text.into(), num: false, path: false, pre: false }
}

fn n(text: impl Into<String>) -> Cell {
    Cell { text: text.into(), num: true, path: false, pre: false }
}

fn p(text: impl Into<String>) -> Cell {
    Cell { text: text.into(), num: false, path: true, pre: false }
}

fn pre(lines: &[String]) -> Cell {
    Cell { text: lines.join("\n"), num: false, path: false, pre: true }
}

fn day(rfc: &str) -> String {
    rfc.chars().take(10).collect()
}

fn notes<T>(s: &Section<T>, blocks: &mut Vec<Block>) {
    if let Some(r) = &s.reason {
        blocks.push(Block::Note(format!("Not available: {r}")));
    }
    if let (true, Some(total)) = (s.truncated, s.total_rows) {
        blocks.push(Block::Note(format!(
            "Showing the first {ROW_CAP} of {total} rows. The JSON and CSV exports have the same cap."
        )));
    }
}

fn days(d: Option<f64>) -> String {
    d.map_or_else(|| "—".into(), |v| if v < 1.0 { "less than a day".into() } else { format!("{v:.0} days") })
}

fn build_sections(r: &Report) -> Vec<SectionView> {
    let s = &r.sections;
    let mut out = Vec::new();

    if let Some(sec) = &s.drives {
        let mut b = Vec::new();
        notes(sec, &mut b);
        if let Some(d) = &sec.data {
            b.push(Block::Svg(charts::usage_bars(d)));
            b.push(Block::Table(TableView {
                headers: vec![
                    h("Drive", false),
                    h("Label", false),
                    h("Type", false),
                    h("Capacity", true),
                    h("Free", true),
                    h("Used", true),
                    h("Health", false),
                ],
                rows: d
                    .iter()
                    .map(|x| {
                        vec![
                            t(&x.drive),
                            t(&x.label),
                            t(format!("{} · {}", x.filesystem, x.media)),
                            n(bytes_label(x.capacity)),
                            n(bytes_label(x.free)),
                            n(format!("{:.1}%", x.used_percent)),
                            t(if x.low_space { format!("{} · low space", x.health) } else { x.health.clone() }),
                        ]
                    })
                    .collect(),
            }));
        }
        out.push(SectionView { id: "drives", title: SectionId::Drives.title(), blocks: b });
    }

    if let Some(sec) = &s.top {
        let mut b = Vec::new();
        notes(sec, &mut b);
        for root in sec.data.iter().flatten() {
            b.push(Block::Heading(format!(
                "{} · scanned {}",
                root.root,
                root.scanned_at.as_deref().map(day).unwrap_or_default()
            )));
            let folders: Vec<(String, u64)> = root.folders.iter().map(|x| (x.path.clone(), x.allocated)).collect();
            if !folders.is_empty() {
                b.push(Block::Svg(charts::treemap(&folders, &format!("Largest folders on {}", root.root))));
            }
            b.push(Block::Table(TableView {
                headers: vec![h("#", true), h("Folder", false), h("Size on disk", true), h("Logical size", true)],
                rows: root
                    .folders
                    .iter()
                    .enumerate()
                    .map(|(i, x)| {
                        vec![n((i + 1).to_string()), p(&x.path), n(bytes_label(x.allocated)), n(bytes_label(x.logical))]
                    })
                    .collect(),
            }));
            let files: Vec<(String, u64)> = root.files.iter().map(|x| (x.path.clone(), x.allocated)).collect();
            if !files.is_empty() {
                b.push(Block::Svg(charts::bars(
                    &files,
                    &format!("Largest files on {}", root.root),
                    palette::CATEGORIES[0].1,
                )));
            }
            b.push(Block::Table(TableView {
                headers: vec![h("#", true), h("File", false), h("Size on disk", true), h("Logical size", true)],
                rows: root
                    .files
                    .iter()
                    .enumerate()
                    .map(|(i, x)| {
                        vec![n((i + 1).to_string()), p(&x.path), n(bytes_label(x.allocated)), n(bytes_label(x.logical))]
                    })
                    .collect(),
            }));
        }
        out.push(SectionView { id: "top", title: SectionId::Top.title(), blocks: b });
    }

    if let Some(sec) = &s.cleanup {
        let mut b = Vec::new();
        notes(sec, &mut b);
        if let Some(c) = &sec.data {
            b.push(Block::Heading("Space you could free, by risk".into()));
            b.push(Block::Table(TableView {
                headers: vec![
                    h("Drive", false),
                    h("Risk", false),
                    h("Kind", false),
                    h("Size on disk", true),
                    h("Matches", true),
                ],
                rows: c
                    .savings
                    .iter()
                    .map(|x| {
                        vec![t(&x.root), t(&x.risk), t(&x.kind), n(bytes_label(x.allocated)), n(x.matches.to_string())]
                    })
                    .collect(),
            }));
            b.push(Block::Heading("Candidates and what to do by hand".into()));
            b.push(Block::Table(TableView {
                headers: vec![
                    h("Candidate", false),
                    h("Risk", false),
                    h("Path", false),
                    h("Size on disk", true),
                    h("Steps (for you to follow)", false),
                ],
                rows: c
                    .candidates
                    .iter()
                    .map(|x| {
                        vec![
                            t(&x.title),
                            t(&x.risk),
                            p(&x.path),
                            n(x.allocated.map_or("—".into(), bytes_label)),
                            pre(&x.instructions),
                        ]
                    })
                    .collect(),
            }));
        }
        out.push(SectionView { id: "cleanup", title: SectionId::Cleanup.title(), blocks: b });
    }

    if let Some(sec) = &s.duplicates {
        let mut b = Vec::new();
        notes(sec, &mut b);
        if let Some(d) = &sec.data {
            b.push(Block::Note(format!(
                "{} groups of identical files; about {} could be freed by keeping one copy of each.",
                d.total_groups,
                bytes_label(d.total_wasted)
            )));
            b.push(Block::Table(TableView {
                headers: vec![h("Group", true), h("Size each", true), h("Wasted", true), h("Copies", false)],
                rows: d
                    .groups
                    .iter()
                    .enumerate()
                    .map(|(i, g)| {
                        let copies: Vec<String> = g
                            .copies
                            .iter()
                            .map(|c| match &c.suggested {
                                Some(s) => format!("{} (suggested: {s})", c.path),
                                None => c.path.clone(),
                            })
                            .collect();
                        vec![
                            n((i + 1).to_string()),
                            n(bytes_label(g.size)),
                            n(bytes_label(g.wasted)),
                            Cell { text: copies.join("\n"), num: false, path: true, pre: false },
                        ]
                    })
                    .collect(),
            }));
        }
        out.push(SectionView { id: "duplicates", title: SectionId::Duplicates.title(), blocks: b });
    }

    if let Some(sec) = &s.stale {
        let mut b = Vec::new();
        notes(sec, &mut b);
        for root in sec.data.iter().flat_map(|d| &d.roots) {
            b.push(Block::Heading(format!(
                "{} · not used for {} months ({})",
                root.root,
                root.months,
                if root.time_basis == "atime" { "last access" } else { "last modified" }
            )));
            b.push(Block::Table(TableView {
                headers: vec![h("Path", false), h("Size on disk", true), h("Last used", false), h("Age", true)],
                rows: root
                    .items
                    .iter()
                    .map(|x| {
                        vec![
                            p(&x.path),
                            n(bytes_label(x.allocated)),
                            t(day(&x.last_used)),
                            n(format!("{} days", x.age_days)),
                        ]
                    })
                    .collect(),
            }));
        }
        out.push(SectionView { id: "stale", title: SectionId::Stale.title(), blocks: b });
    }

    if let Some(sec) = &s.tiering {
        let mut b = Vec::new();
        notes(sec, &mut b);
        if let Some(ti) = &sec.data {
            for note in &ti.notes {
                b.push(Block::Note(note.clone()));
            }
            b.push(Block::Table(TableView {
                headers: vec![
                    h("Drive", false),
                    h("Capacity", true),
                    h("Free now", true),
                    h("Free after planned moves", true),
                ],
                rows: ti
                    .drives
                    .iter()
                    .map(|d| {
                        vec![
                            t(&d.drive),
                            n(bytes_label(d.capacity)),
                            n(bytes_label(d.free)),
                            n(bytes_label(d.projected_free)),
                        ]
                    })
                    .collect(),
            }));
            b.push(Block::Table(TableView {
                headers: vec![
                    h("Move", false),
                    h("Size on disk", true),
                    h("From → to", false),
                    h("Why", false),
                    h("Steps (for you to follow)", false),
                ],
                rows: ti
                    .suggestions
                    .iter()
                    .map(|x| {
                        vec![
                            p(&x.path),
                            n(bytes_label(x.allocated)),
                            t(format!("{} → {}", x.from_drive, x.to_drive.as_deref().unwrap_or("?"))),
                            t(&x.reason),
                            pre(&x.instructions),
                        ]
                    })
                    .collect(),
            }));
        }
        out.push(SectionView { id: "tiering", title: SectionId::Tiering.title(), blocks: b });
    }

    if let Some(sec) = &s.plans {
        let mut b = Vec::new();
        notes(sec, &mut b);
        for plan in sec.data.iter().flat_map(|d| &d.plans) {
            b.push(Block::Heading(format!(
                "{} · created {} · {} verified as freed",
                plan.name,
                day(&plan.created_at),
                bytes_label(plan.freed_total.max(0) as u64)
            )));
            b.push(Block::Table(TableView {
                headers: vec![
                    h("Item", false),
                    h("Risk", false),
                    h("Size on disk", true),
                    h("Status", false),
                    h("Steps (for you to follow)", false),
                ],
                rows: plan
                    .items
                    .iter()
                    .map(|i| {
                        vec![
                            p(&i.path),
                            t(&i.risk),
                            n(i.allocated.map_or("—".into(), bytes_label)),
                            t(&i.status),
                            pre(&i.instructions),
                        ]
                    })
                    .collect(),
            }));
        }
        out.push(SectionView { id: "plans", title: SectionId::Plans.title(), blocks: b });
    }

    if let Some(sec) = &s.forecast {
        let mut b = Vec::new();
        notes(sec, &mut b);
        if let Some(f) = &sec.data {
            b.push(Block::Note("The forecast extends the recent trend in free space. It is an estimate, not a promise; it is not shown until there are at least 3 samples over 7 days.".into()));
            for v in &f.volumes {
                b.push(Block::Heading(v.drive.clone()));
                b.push(Block::Svg(charts::forecast_line(v, &format!("Free space of {} over time", v.drive))));
            }
            b.push(Block::Table(TableView {
                headers: vec![
                    h("Drive", false),
                    h("Outlook", false),
                    h("Low in", true),
                    h("Full in", true),
                    h("Loses per day", true),
                    h("Confidence", false),
                ],
                rows: f
                    .volumes
                    .iter()
                    .map(|v| {
                        vec![
                            t(&v.drive),
                            t(match v.state.as_str() {
                                "filling" => "Free space is shrinking",
                                "not-filling" => "Stable",
                                _ => "Not enough history yet",
                            }),
                            n(days(v.days_to_low)),
                            n(days(v.days_to_full)),
                            n(v.bytes_per_day.map_or("—".into(), |x| bytes_label(x as u64))),
                            t(&v.confidence),
                        ]
                    })
                    .collect(),
            }));
        }
        out.push(SectionView { id: "forecast", title: SectionId::Forecast.title(), blocks: b });
    }
    out
}

/// Render the report as one self-contained HTML document.
pub fn render(r: &Report) -> Result<Vec<u8>, String> {
    let page = Page {
        version: r.app_version,
        scope: r.scope.drive.clone().map_or_else(|| "All drives".to_string(), |d| format!("Drive {d}")),
        generated_at: &r.generated_at,
        sections: build_sections(r),
    };
    page.render().map(String::into_bytes).map_err(|e| e.to_string())
}

#[cfg(test)]
#[path = "html_tests.rs"]
mod tests;

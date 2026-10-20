//! Inline SVG charts for the HTML report. Pure string building: no scripts, no
//! external assets, no `xmlns` (an HTML5 document does not need one, and it
//! keeps every `http://` string out of the file).

use std::fmt::Write;

use super::model::{DriveRow, ForecastRow};

/// Category and state colours: the light-theme values of `frontend/src/styles/tokens.css`.
/// A test reads that file and fails when they drift apart.
pub mod palette {
    pub const CATEGORIES: [(&str, &str); 10] = [
        ("video", "#346ad6"),
        ("images", "#8e5ff0"),
        ("audio", "#d6488a"),
        ("archives", "#c58400"),
        ("installers", "#d9682c"),
        ("documents", "#6b8a2a"),
        ("code", "#1e9e63"),
        ("disk-images", "#1c93ad"),
        ("system", "#6b7280"),
        ("other", "#7c8490"),
    ];
    pub const FILL_NORMAL: &str = "#0b7a70";
    pub const FILL_CAUTION: &str = "#c58400";
    pub const FILL_WARNING: &str = "#d4572f";
    pub const FILL_TRACK: &str = "#e6e9ed";
    pub const FOREGROUND: &str = "#14171c";
    pub const MUTED: &str = "#5f6772";
    pub const BORDER: &str = "#e1e4e9";
}

const GIB: f64 = (1u64 << 30) as f64;

pub fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            '\'' => o.push_str("&#39;"),
            c => o.push(c),
        }
    }
    o
}

/// "1.5 GiB" style size, like the dashboard.
pub fn bytes_label(b: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut v = b as f64;
    let mut i = 0;
    while v >= 1024.0 && i < UNITS.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{b} B")
    } else if v >= 100.0 {
        format!("{v:.0} {}", UNITS[i])
    } else if v >= 10.0 {
        format!("{v:.1} {}", UNITS[i])
    } else {
        format!("{v:.2} {}", UNITS[i])
    }
}

fn clip(s: &str, max: usize) -> String {
    let n = s.chars().count();
    if n <= max {
        return s.to_string();
    }
    // Keep the end of a path: it is the part that tells folders apart.
    let tail: String = s.chars().skip(n - (max - 1)).collect();
    format!("…{tail}")
}

fn svg_open(w: f64, h: f64, label: &str) -> String {
    format!(
        "<svg viewBox=\"0 0 {w} {h}\" width=\"{w}\" height=\"{h}\" role=\"img\" aria-label=\"{}\" style=\"max-width:100%;height:auto\"><title>{}</title>",
        esc(label),
        esc(label)
    )
}

/// One bar per drive: used share, coloured by how full it is.
pub fn usage_bars(drives: &[DriveRow]) -> String {
    let row_h = 34.0;
    let (w, h) = (680.0, row_h * drives.len() as f64 + 8.0);
    let mut s = svg_open(w, h, "Used and free space per drive");
    for (i, d) in drives.iter().enumerate() {
        let y = 4.0 + row_h * i as f64;
        let used = if d.capacity > 0 { (d.used as f64 / d.capacity as f64).clamp(0.0, 1.0) } else { 0.0 };
        let colour = if d.low_space {
            palette::FILL_WARNING
        } else if used >= 0.75 {
            palette::FILL_CAUTION
        } else {
            palette::FILL_NORMAL
        };
        let _ = write!(
            s,
            "<text x=\"0\" y=\"{ty}\" font-size=\"13\" fill=\"{fg}\">{label}</text>\
             <rect x=\"150\" y=\"{y}\" width=\"300\" height=\"14\" rx=\"7\" fill=\"{track}\"/>\
             <rect x=\"150\" y=\"{y}\" width=\"{bw:.1}\" height=\"14\" rx=\"7\" fill=\"{colour}\"/>\
             <text x=\"460\" y=\"{ty}\" font-size=\"12\" fill=\"{muted}\">{free} free of {cap} ({pct:.0}% used)</text>",
            ty = y + 12.0,
            fg = palette::FOREGROUND,
            label = esc(&clip(&format!("{} {}", d.drive, d.label), 18)),
            track = palette::FILL_TRACK,
            bw = 300.0 * used,
            muted = palette::MUTED,
            free = bytes_label(d.free),
            cap = bytes_label(d.capacity),
            pct = used * 100.0,
        );
    }
    s.push_str("</svg>");
    s
}

/// Horizontal bars for a ranked list, longest first.
pub fn bars(items: &[(String, u64)], label: &str, colour: &str) -> String {
    let items = &items[..items.len().min(10)];
    let row_h = 26.0;
    let (w, h) = (680.0, row_h * items.len() as f64 + 8.0);
    let max = items.iter().map(|i| i.1).max().unwrap_or(1).max(1) as f64;
    let mut s = svg_open(w, h, label);
    for (i, (name, v)) in items.iter().enumerate() {
        let y = 4.0 + row_h * i as f64;
        let _ = write!(
            s,
            "<text x=\"0\" y=\"{ty}\" font-size=\"12\" fill=\"{fg}\">{name}</text>\
             <rect x=\"300\" y=\"{y}\" width=\"{bw:.1}\" height=\"14\" rx=\"3\" fill=\"{colour}\"/>\
             <text x=\"{vx:.1}\" y=\"{ty}\" font-size=\"12\" fill=\"{muted}\">{value}</text>",
            ty = y + 12.0,
            fg = palette::FOREGROUND,
            name = esc(&clip(name, 44)),
            bw = (*v as f64 / max * 280.0).max(2.0),
            vx = 300.0 + (*v as f64 / max * 280.0).max(2.0) + 6.0,
            muted = palette::MUTED,
            value = bytes_label(*v),
        );
    }
    s.push_str("</svg>");
    s
}

#[derive(Clone, Copy)]
struct Rect {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

/// Squarified treemap of up to 20 items: area is proportional to size.
pub fn treemap(items: &[(String, u64)], label: &str) -> String {
    let items: Vec<&(String, u64)> = items.iter().filter(|i| i.1 > 0).take(20).collect();
    let (w, h) = (680.0, 300.0);
    let mut s = svg_open(w, h, label);
    let total: f64 = items.iter().map(|i| i.1 as f64).sum();
    if total > 0.0 {
        let areas: Vec<f64> = items.iter().map(|i| i.1 as f64 / total * w * h).collect();
        let rects = squarify(&areas, Rect { x: 0.0, y: 0.0, w, h });
        for (i, (item, r)) in items.iter().zip(rects).enumerate() {
            let colour = palette::CATEGORIES[i % palette::CATEGORIES.len()].1;
            let _ = write!(
                s,
                "<rect x=\"{:.1}\" y=\"{:.1}\" width=\"{:.1}\" height=\"{:.1}\" fill=\"{colour}\" stroke=\"#ffffff\" stroke-width=\"2\"><title>{} ({})</title></rect>",
                r.x,
                r.y,
                (r.w - 1.0).max(0.0),
                (r.h - 1.0).max(0.0),
                esc(&item.0),
                bytes_label(item.1),
            );
            if r.w > 90.0 && r.h > 34.0 {
                let name = item.0.rsplit('\\').next().unwrap_or(&item.0);
                let _ = write!(
                    s,
                    "<text x=\"{:.1}\" y=\"{:.1}\" font-size=\"12\" fill=\"#ffffff\">{}</text><text x=\"{:.1}\" y=\"{:.1}\" font-size=\"11\" fill=\"#ffffff\">{}</text>",
                    r.x + 6.0,
                    r.y + 16.0,
                    esc(&clip(name, ((r.w - 12.0) / 7.0) as usize)),
                    r.x + 6.0,
                    r.y + 30.0,
                    bytes_label(item.1),
                );
            }
        }
    }
    s.push_str("</svg>");
    s
}

fn worst(row: &[f64], side: f64) -> f64 {
    let sum: f64 = row.iter().sum();
    let (mn, mx) = row.iter().fold((f64::MAX, 0.0f64), |(a, b), &v| (a.min(v), b.max(v)));
    if sum <= 0.0 || mn <= 0.0 {
        return f64::MAX;
    }
    ((side * side * mx) / (sum * sum)).max((sum * sum) / (side * side * mn))
}

fn squarify(areas: &[f64], mut r: Rect) -> Vec<Rect> {
    let mut out = Vec::with_capacity(areas.len());
    let mut i = 0;
    while i < areas.len() {
        let side = r.w.min(r.h);
        let mut row = vec![areas[i]];
        let mut j = i + 1;
        while j < areas.len() {
            let mut next = row.clone();
            next.push(areas[j]);
            if worst(&next, side) <= worst(&row, side) {
                row = next;
                j += 1;
            } else {
                break;
            }
        }
        let sum: f64 = row.iter().sum();
        if r.w >= r.h {
            let col_w = sum / r.h;
            let mut y = r.y;
            for a in &row {
                let hh = a / col_w;
                out.push(Rect { x: r.x, y, w: col_w, h: hh });
                y += hh;
            }
            r = Rect { x: r.x + col_w, y: r.y, w: r.w - col_w, h: r.h };
        } else {
            let row_h = sum / r.w;
            let mut x = r.x;
            for a in &row {
                let ww = a / row_h;
                out.push(Rect { x, y: r.y, w: ww, h: row_h });
                x += ww;
            }
            r = Rect { x: r.x, y: r.y + row_h, w: r.w, h: r.h - row_h };
        }
        i = j;
    }
    out
}

/// Free space over time, with the forecast as a dashed line.
pub fn forecast_line(v: &ForecastRow, label: &str) -> String {
    let (w, h) = (680.0, 200.0);
    let (left, right, top, bottom) = (64.0, 16.0, 12.0, 28.0);
    let mut s = svg_open(w, h, label);
    if v.samples.len() >= 2 && v.capacity > 0 {
        let (t0, t1) = (v.samples[0][0] as f64, v.samples[v.samples.len() - 1][0] as f64);
        // Room on the right for the forecast: up to 60 days ahead.
        let horizon = if v.state == "filling" { v.days_to_full.unwrap_or(0.0).min(60.0) * 86_400.0 } else { 0.0 };
        let t_end = (t1 + horizon).max(t0 + 1.0);
        let x = |t: f64| left + (t - t0) / (t_end - t0) * (w - left - right);
        let y = |f: f64| top + (1.0 - (f / v.capacity as f64).clamp(0.0, 1.0)) * (h - top - bottom);
        let _ = write!(
            s,
            "<line x1=\"{left}\" y1=\"{b}\" x2=\"{r}\" y2=\"{b}\" stroke=\"{border}\"/><line x1=\"{left}\" y1=\"{top}\" x2=\"{left}\" y2=\"{b}\" stroke=\"{border}\"/>\
             <text x=\"4\" y=\"{ty}\" font-size=\"11\" fill=\"{muted}\">{cap}</text><text x=\"4\" y=\"{b}\" font-size=\"11\" fill=\"{muted}\">0</text>",
            b = h - bottom,
            r = w - right,
            border = palette::BORDER,
            ty = top + 10.0,
            muted = palette::MUTED,
            cap = bytes_label(v.capacity),
        );
        let points: Vec<String> =
            v.samples.iter().map(|[t, f]| format!("{:.1},{:.1}", x(*t as f64), y(*f as f64))).collect();
        let _ = write!(
            s,
            "<polyline points=\"{}\" fill=\"none\" stroke=\"{}\" stroke-width=\"2\"/>",
            points.join(" "),
            palette::FILL_NORMAL
        );
        if let (true, Some(rate), Some(days)) = (v.state == "filling", v.bytes_per_day, v.days_to_full) {
            let last = v.samples[v.samples.len() - 1];
            let days = days.min(60.0);
            let end_free = (last[1] as f64 - rate * days).max(0.0);
            let _ = write!(
                s,
                "<line x1=\"{:.1}\" y1=\"{:.1}\" x2=\"{:.1}\" y2=\"{:.1}\" stroke=\"{}\" stroke-width=\"2\" stroke-dasharray=\"6 4\"/>",
                x(last[0] as f64),
                y(last[1] as f64),
                x(t1 + days * 86_400.0),
                y(end_free),
                palette::FILL_WARNING,
            );
        }
        let _ = write!(
            s,
            "<text x=\"{left}\" y=\"{y}\" font-size=\"11\" fill=\"{muted}\">{a}</text><text x=\"{rx}\" y=\"{y}\" font-size=\"11\" text-anchor=\"end\" fill=\"{muted}\">{b}</text>",
            y = h - 8.0,
            rx = w - right,
            muted = palette::MUTED,
            a = date(t0 as i64),
            b = date(t_end as i64),
        );
    } else {
        let _ = write!(
            s,
            "<text x=\"12\" y=\"100\" font-size=\"13\" fill=\"{}\">Not enough history to draw a chart yet.</text>",
            palette::MUTED
        );
    }
    s.push_str("</svg>");
    s
}

fn date(ts: i64) -> String {
    humantime::format_rfc3339_seconds(std::time::UNIX_EPOCH + std::time::Duration::from_secs(ts.max(0) as u64))
        .to_string()
        .chars()
        .take(10)
        .collect()
}

/// Whole GiB, for text that wants a rounded figure.
pub fn gib(b: u64) -> f64 {
    b as f64 / GIB
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drive(letter: &str, cap_gib: u64, free_gib: u64, low: bool) -> DriveRow {
        DriveRow {
            drive: letter.into(),
            label: "Data <&>".into(),
            filesystem: "NTFS".into(),
            media: "ssd".into(),
            capacity: cap_gib << 30,
            free: free_gib << 30,
            used: (cap_gib - free_gib) << 30,
            used_percent: 0.0,
            health: "healthy".into(),
            low_space: low,
        }
    }

    #[test]
    fn text_is_escaped_and_nothing_external_is_referenced() {
        let svg = usage_bars(&[drive("C:", 500, 100, false), drive("D:", 931, 40, true)]);
        assert!(svg.contains("Data &lt;&amp;&gt;"), "{svg}");
        assert!(!svg.contains("<script") && !svg.contains("http") && !svg.contains("href") && !svg.contains("xmlns"));
        assert!(svg.contains(palette::FILL_WARNING), "the low drive is coloured as a warning");
        let b =
            bars(&[("D:\\a\"b".into(), 5 << 30), ("D:\\c".into(), 1 << 30)], "Largest folders", palette::FILL_NORMAL);
        assert!(b.contains("&quot;") && !b.contains("a\"b"));
    }

    #[test]
    fn bytes_are_labelled_like_the_dashboard() {
        assert_eq!(bytes_label(512), "512 B");
        assert_eq!(bytes_label(1 << 30), "1.00 GiB");
        assert_eq!(bytes_label(15 << 30), "15.0 GiB");
        assert_eq!(bytes_label(200 << 20), "200 MiB");
        assert_eq!(clip("D:\\a\\very\\long\\path\\name", 10), "…path\\name");
    }

    #[test]
    fn the_treemap_fills_the_area_without_overlap() {
        let items: Vec<(String, u64)> = (1..=25).map(|i| (format!("D:\\f{i}"), (26 - i) * 100)).collect();
        let svg = treemap(&items, "Top folders");
        assert_eq!(svg.matches("<rect").count(), 20, "at most 20 tiles");
        let areas: Vec<f64> = (1..=20).map(|i| (21 - i) as f64).collect();
        let total: f64 = areas.iter().sum();
        let scaled: Vec<f64> = areas.iter().map(|a| a / total * 680.0 * 300.0).collect();
        let rects = squarify(&scaled, Rect { x: 0.0, y: 0.0, w: 680.0, h: 300.0 });
        let covered: f64 = rects.iter().map(|r| r.w * r.h).sum();
        assert!((covered - 680.0 * 300.0).abs() < 1.0, "{covered}");
        for (a, r) in scaled.iter().zip(&rects) {
            assert!((r.w * r.h - a).abs() < 0.5, "tile area follows size");
            assert!(r.x >= -0.01 && r.y >= -0.01 && r.x + r.w <= 680.01 && r.y + r.h <= 300.01);
        }
        assert!(treemap(&[], "x").contains("</svg>"));
    }

    #[test]
    fn the_forecast_line_draws_history_and_a_dashed_projection() {
        let v = ForecastRow {
            drive: "D:".into(),
            capacity: 500 << 30,
            state: "filling".into(),
            days_to_low: Some(20.0),
            days_to_full: Some(50.0),
            bytes_per_day: Some((1u64 << 30) as f64),
            confidence: "high".into(),
            samples: (0..30).map(|d| [1_700_000_000 + d * 86_400, (80 - d) << 30]).collect(),
        };
        let svg = forecast_line(&v, "Free space of D:");
        assert!(svg.contains("<polyline") && svg.contains("stroke-dasharray"));
        let none = forecast_line(&ForecastRow { samples: vec![], ..v }, "x");
        assert!(none.contains("Not enough history"));
    }

    #[test]
    fn the_palette_matches_the_dashboard_tokens() {
        let css = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../frontend/src/styles/tokens.css"))
            .expect("tokens.css");
        // The light theme is the first block, before the dark overrides.
        let light = css.split("[data-theme=\"dark\"]").next().unwrap_or(&css);
        let light = light.split("prefers-color-scheme").next().unwrap_or(light);
        for (name, hex) in palette::CATEGORIES {
            assert!(light.contains(&format!("--cat-{name}: {hex};")), "--cat-{name} should be {hex}");
        }
        for (name, hex) in [
            ("fill-normal", palette::FILL_NORMAL),
            ("fill-caution", palette::FILL_CAUTION),
            ("fill-warning", palette::FILL_WARNING),
            ("fill-track", palette::FILL_TRACK),
        ] {
            assert!(light.contains(&format!("--{name}: {hex};")), "--{name} should be {hex}");
        }
    }
}

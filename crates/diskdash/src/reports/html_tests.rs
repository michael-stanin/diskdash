use super::*;

fn full_report() -> Report {
    let evil = "<script>alert(1)</script> & \"quoted\"".to_string();
    let instr = vec![r"Remove-Item -Recurse 'C:\Temp\x'".to_string(), evil.clone()];
    let s = Sections {
        drives: Some(Section::ok(vec![DriveRow {
            drive: "C:".into(),
            label: evil.clone(),
            filesystem: "NTFS".into(),
            media: "ssd".into(),
            capacity: 500 << 30,
            free: 40 << 30,
            used: 460 << 30,
            used_percent: 92.0,
            health: "healthy".into(),
            low_space: true,
        }])),
        top: Some(Section::ok(vec![TopRoot {
            root: r"C:\".into(),
            snapshot_id: 1,
            scanned_at: Some("2026-10-01T10:00:00Z".into()),
            folders: vec![
                TopEntry { path: r"C:\Users".into(), allocated: 90 << 30, logical: 80 << 30 },
                TopEntry { path: r"C:\Windows".into(), allocated: 30 << 30, logical: 30 << 30 },
            ],
            files: vec![TopEntry { path: r"C:\pagefile.sys".into(), allocated: 16 << 30, logical: 16 << 30 }],
        }])),
        cleanup: Some(
            Section::ok(CleanupSection {
                savings: vec![SavingRow {
                    root: r"C:\".into(),
                    risk: "safe".into(),
                    kind: "manual-cleanup".into(),
                    allocated: 5 << 30,
                    logical: 5 << 30,
                    matches: 3,
                }],
                candidates: vec![CandidateRow {
                    root: r"C:\".into(),
                    rule_id: "temp".into(),
                    title: evil.clone(),
                    risk: "safe".into(),
                    kind: "manual-cleanup".into(),
                    path: r"C:\Temp".into(),
                    allocated: Some(5 << 30),
                    logical: None,
                    instructions: instr.clone(),
                }],
            })
            .cut(250, 100),
        ),
        duplicates: Some(Section::ok(DuplicatesSection {
            total_groups: 1,
            total_wasted: 1 << 30,
            groups: vec![DupRow {
                size: 1 << 30,
                wasted: 1 << 30,
                copies: vec![
                    DupCopy { path: r"C:\a.iso".into(), suggested: Some("keep".into()) },
                    DupCopy { path: r"D:\a.iso".into(), suggested: Some("remove".into()) },
                ],
            }],
        })),
        stale: Some(Section::ok(StaleSection {
            roots: vec![StaleRoot {
                root: r"D:\".into(),
                months: 12,
                time_basis: "atime".into(),
                items: vec![StaleRow {
                    path: r"D:\old".into(),
                    allocated: 2 << 30,
                    last_used: "2024-01-01T00:00:00Z".into(),
                    age_days: 600,
                }],
            }],
        })),
        tiering: Some(Section::ok(TieringSection {
            as_of: None,
            drives: vec![TierDriveRow {
                drive: "C:".into(),
                capacity: 500 << 30,
                free: 40 << 30,
                projected_free: 80 << 30,
            }],
            suggestions: vec![TierRow {
                path: r"C:\Games\x".into(),
                direction: "ssd-to-hdd".into(),
                kind: "app-library".into(),
                allocated: 40 << 30,
                from_drive: "C:".into(),
                to_drive: Some("D:".into()),
                destination: Some(r"D:\Games\x".into()),
                reason: "not played for 14 months".into(),
                instructions: instr.clone(),
            }],
            notes: vec![evil.clone()],
        })),
        plans: Some(Section::ok(PlansSection {
            plans: vec![PlanRow {
                name: evil,
                created_at: "2026-10-02T09:00:00Z".into(),
                freed_total: 1 << 30,
                items: vec![PlanItemRow {
                    path: r"C:\Temp".into(),
                    title: "Temp".into(),
                    risk: "safe".into(),
                    allocated: Some(5 << 30),
                    status: "todo".into(),
                    destination: None,
                    instructions: instr,
                }],
            }],
        })),
        forecast: Some(Section::ok(ForecastSection {
            volumes: vec![ForecastRow {
                drive: "C:".into(),
                capacity: 500 << 30,
                state: "filling".into(),
                days_to_low: Some(10.0),
                days_to_full: Some(40.0),
                bytes_per_day: Some((1u64 << 30) as f64),
                confidence: "high".into(),
                samples: (0..20).map(|d| [1_700_000_000 + d * 86_400, (80 - d) << 30]).collect(),
            }],
        })),
    };
    Report {
        schema_version: SCHEMA_VERSION,
        generated_at: "2026-10-20T10:00:00Z".into(),
        app_version: "0.1.0",
        scope: Scope { drive: None },
        top_n: 25,
        sections: s,
    }
}

#[test]
fn every_section_renders_with_hostile_text_escaped_and_nothing_external() {
    let html = String::from_utf8(render(&full_report()).unwrap()).unwrap();
    for id in ["drives", "top", "cleanup", "duplicates", "stale", "tiering", "plans", "forecast"] {
        assert!(html.contains(&format!("id=\"{id}\"")), "{id}");
    }
    assert!(!html.contains("<script"), "text from disk or rules can never become markup");
    assert!(
        html.contains("&lt;script&gt;alert(1)&lt;/script&gt; &amp; &quot;quoted&quot;")
            || html.contains("&lt;script&gt;alert(1)&lt;/script&gt; &amp; &#34;quoted&#34;"),
        "escaped, not dropped"
    );
    for needle in ["http://", "https://", "<link", "<img", " src=", "@import", "url(", "xmlns"] {
        assert!(!html.contains(needle), "{needle}");
    }
    // Instructions are text inside <pre>, never anything runnable.
    assert!(html.contains("<pre>Remove-Item -Recurse"), "commands shown as text");
    assert!(html.contains("Showing the first 100 of 250 rows"), "truncation is stated");
    assert!(html.contains("stroke-dasharray"), "forecast chart has the projection");
    assert!(html.contains("(suggested: keep)"));
}

#[test]
fn a_report_with_no_sections_still_renders() {
    let r = Report { sections: Sections::default(), ..full_report() };
    let html = String::from_utf8(render(&r).unwrap()).unwrap();
    assert!(html.contains("diskdash report") && !html.contains("<section"));
}

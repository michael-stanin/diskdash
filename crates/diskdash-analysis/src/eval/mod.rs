//! Rule evaluation over one scan snapshot.
//!
//! Matches are derived from the snapshot's `entries` only. The single
//! exceptions are the [`probe::Probe`] metadata queries for the Recycle Bin,
//! shadow storage and a few well-known system files. No user file is ever
//! opened: evaluation reads the snapshot database and writes its results
//! (`rule_matches`) back into that same database.

pub mod probe;
pub mod protected;

use std::cell::RefCell;
use std::collections::HashMap;

use globset::{GlobBuilder, GlobMatcher};
use rusqlite::{params, Connection, OptionalExtension};

use crate::rules::expand::expand;
use crate::rules::{AppliesTo, Conditions, Env, LoadedRule, Risk, RuleDef, RuleKind, VirtualSource};
use probe::{Probe, ProbeError};
use protected::Protected;

const SCHEMA: &str = include_str!("schema.sql");

#[derive(Debug, thiserror::Error)]
pub enum EvalError {
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    #[error("cancelled")]
    Cancelled,
    #[error("{0}")]
    Snapshot(String),
}

pub struct EvalOptions<'a> {
    pub env: &'a dyn Env,
    pub probe: &'a dyn Probe,
    /// Unix seconds; ages are measured against this.
    pub now: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleFailure {
    pub rule_id: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Saving {
    pub risk: Risk,
    pub kind: RuleKind,
    pub allocated: u64,
    pub logical: u64,
    pub matches: u64,
    /// Matches whose size could not be measured.
    pub unknown: u64,
}

#[derive(Debug, Default)]
pub struct EvalSummary {
    pub rules_evaluated: usize,
    pub rule_failures: Vec<RuleFailure>,
    /// Every match, including nested ones.
    pub matches: usize,
    /// Matches that count towards savings.
    pub counted: usize,
    pub savings: Vec<Saving>,
}

#[derive(Debug, Clone)]
struct Row {
    id: i64,
    parent_id: Option<i64>,
    name: String,
    kind: i64,
    size: i64,
    alloc: i64,
    mtime: Option<i64>,
    max_mtime: Option<i64>,
    flags: i64,
    last_desc: i64,
    file_count: Option<i64>,
    path: Option<String>,
}

const COLS: &str =
    "id, parent_id, name, kind, size, alloc, mtime, max_mtime, flags, coalesce(last_desc_id, id), file_count, path";

fn row_from(r: &rusqlite::Row<'_>) -> rusqlite::Result<Row> {
    Ok(Row {
        id: r.get(0)?,
        parent_id: r.get(1)?,
        name: r.get(2)?,
        kind: r.get(3)?,
        size: r.get(4)?,
        alloc: r.get(5)?,
        mtime: r.get(6)?,
        max_mtime: r.get(7)?,
        flags: r.get(8)?,
        last_desc: r.get(9)?,
        file_count: r.get(10)?,
        path: r.get(11)?,
    })
}

impl Row {
    fn is_dir(&self) -> bool {
        self.kind == 1
    }

    fn cloud_placeholder(&self) -> bool {
        self.flags & 2 != 0
    }

    fn access_denied(&self) -> bool {
        self.flags & 1 != 0
    }
}

/// One thing a rule found.
#[derive(Debug, Clone)]
struct Cand {
    row: Option<Row>,
    virtual_key: Option<&'static str>,
    path: String,
    is_dir: bool,
    logical: Option<u64>,
    allocated: Option<u64>,
    file_count: Option<u64>,
    modified: Option<i64>,
    note: Option<String>,
}

impl Cand {
    fn from_row(row: Row, path: String) -> Self {
        let note = (row.is_dir() && row.access_denied())
            .then(|| "Some folders inside could not be read, so the real size may be larger.".to_string());
        Cand {
            is_dir: row.is_dir(),
            logical: Some(row.size.max(0) as u64),
            allocated: Some(row.alloc.max(0) as u64),
            file_count: row.file_count.map(|n| n.max(0) as u64),
            modified: if row.is_dir() { row.max_mtime.or(row.mtime) } else { row.mtime },
            note,
            virtual_key: None,
            path,
            row: Some(row),
        }
    }

    fn virtual_item(key: &'static str, path: String, bytes: Option<u64>, note: Option<String>) -> Self {
        Cand {
            row: None,
            virtual_key: Some(key),
            path,
            is_dir: false,
            logical: bytes,
            allocated: bytes,
            file_count: None,
            modified: None,
            note,
        }
    }
}

// ---------------------------------------------------------------------------
// Glob planning
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
enum NameFilter {
    Literal(String),
    Prefix(String),
    Suffix(String),
    /// Anything else: matched in Rust against every entry name.
    Other,
}

struct GlobPlan {
    full: GlobMatcher,
    name: NameFilter,
    name_matcher: GlobMatcher,
}

const META: &[char] = &['*', '?', '[', ']', '{', '}'];

fn name_filter(last: &str) -> NameFilter {
    let l = last.to_lowercase();
    if !l.contains(META) {
        NameFilter::Literal(l)
    } else if let Some(s) = l.strip_prefix('*').filter(|s| !s.contains(META)) {
        NameFilter::Suffix(s.to_string())
    } else if let Some(p) = l.strip_suffix('*').filter(|p| !p.contains(META)) {
        NameFilter::Prefix(p.to_string())
    } else {
        NameFilter::Other
    }
}

/// Escape for SQLite GLOB.
fn glob_escape(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '*' | '?' | '[' => format!("[{c}]"),
            c => c.to_string(),
        })
        .collect()
}

fn plan(glob: &str) -> Result<GlobPlan, String> {
    let mut g = glob.replace('\\', "/");
    // Like .gitignore: a pattern without a slash matches at any depth.
    if !g.contains('/') {
        g = format!("**/{g}");
    }
    let build = |p: &str| {
        GlobBuilder::new(p)
            .case_insensitive(true)
            .literal_separator(true)
            .build()
            .map(|g| g.compile_matcher())
            .map_err(|e| e.to_string())
    };
    let last = g.rsplit('/').next().unwrap_or(&g).to_string();
    Ok(GlobPlan { full: build(&g)?, name: name_filter(&last), name_matcher: build(&last)? })
}

// ---------------------------------------------------------------------------
// Matching context
// ---------------------------------------------------------------------------

const ARCHIVE_EXTS: &[&str] = &["zip", "7z", "rar", "tar", "gz", "tgz", "bz2", "xz"];

struct Ctx<'a> {
    conn: &'a Connection,
    /// Snapshot root, e.g. `C:\` or `D:\Projects`.
    root: String,
    root_trimmed_lower: String,
    drive_root: bool,
    system_drive: Option<String>,
    opts: &'a EvalOptions<'a>,
    protected: Protected,
    dir_paths: RefCell<HashMap<i64, String>>,
}

fn within_rel<'p>(path: &'p str, base_trimmed_lower: &str) -> Option<&'p str> {
    let lower = path.to_lowercase();
    if lower == base_trimmed_lower {
        return Some("");
    }
    // `to_lowercase` can change byte lengths for exotic characters; compare on chars.
    let rest = lower.strip_prefix(base_trimmed_lower)?.strip_prefix('\\')?;
    let consumed = path.len().checked_sub(rest.len())?;
    path.get(consumed..)
}

impl<'a> Ctx<'a> {
    fn new(conn: &'a Connection, opts: &'a EvalOptions<'a>) -> Result<Self, EvalError> {
        let root: String = conn
            .query_row("SELECT path FROM entries WHERE id = 0", [], |r| r.get::<_, Option<String>>(0))
            .optional()?
            .flatten()
            .ok_or_else(|| EvalError::Snapshot("the snapshot has no root folder".into()))?;
        let root_trimmed_lower = root.trim_end_matches('\\').to_lowercase();
        let drive_root = root.len() <= 3 && root.ends_with('\\');
        let system_drive =
            opts.env.var("SystemDrive").map(|d| format!("{}\\", d.trim_end_matches('\\').to_uppercase()));
        Ok(Self {
            conn,
            root,
            root_trimmed_lower,
            drive_root,
            system_drive,
            opts,
            protected: Protected::new(opts.env),
            dir_paths: RefCell::new(HashMap::new()),
        })
    }

    fn rel_to_root<'p>(&self, path: &'p str) -> Option<&'p str> {
        within_rel(path, &self.root_trimmed_lower)
    }

    fn row_by_id(&self, id: i64) -> rusqlite::Result<Option<Row>> {
        self.conn
            .prepare_cached(&format!("SELECT {COLS} FROM entries WHERE id = ?1"))?
            .query_row([id], row_from)
            .optional()
    }

    /// Directory path of entry `id` (cached).
    fn dir_path(&self, id: i64) -> rusqlite::Result<String> {
        if let Some(p) = self.dir_paths.borrow().get(&id) {
            return Ok(p.clone());
        }
        let p: String = self
            .conn
            .prepare_cached("SELECT coalesce(path, '') FROM entries WHERE id = ?1")?
            .query_row([id], |r| r.get(0))
            .optional()?
            .unwrap_or_default();
        self.dir_paths.borrow_mut().insert(id, p.clone());
        Ok(p)
    }

    fn path_of(&self, row: &Row) -> rusqlite::Result<String> {
        if row.is_dir() {
            return Ok(row.path.clone().unwrap_or_default());
        }
        let parent = match row.parent_id {
            Some(p) => self.dir_path(p)?,
            None => String::new(),
        };
        Ok(diskdash_path_join(&parent, &row.name))
    }

    /// Resolve an absolute path to its entry by walking name by name.
    fn resolve(&self, abs: &str) -> rusqlite::Result<Option<Row>> {
        let Some(rel) = self.rel_to_root(abs) else { return Ok(None) };
        let mut cur = match self.row_by_id(0)? {
            Some(r) => r,
            None => return Ok(None),
        };
        for seg in rel.split('\\').filter(|s| !s.is_empty()) {
            let next = self
                .conn
                .prepare_cached(&format!(
                    "SELECT {COLS} FROM entries WHERE parent_id = ?1 AND name_lower = ?2 AND id != 0 LIMIT 1"
                ))?
                .query_row(params![cur.id, seg.to_lowercase()], row_from)
                .optional()?;
            match next {
                Some(r) => cur = r,
                None => return Ok(None),
            }
        }
        Ok(Some(cur))
    }

    fn named_child_exists(&self, parent: i64, pats: &[String], dirs_only: bool) -> rusqlite::Result<bool> {
        for pat in pats {
            let p = pat.to_lowercase();
            let kind_clause = if dirs_only { "AND kind = 1" } else { "" };
            let found: Option<i64> = if p.contains(['*', '?']) {
                self.conn
                    .prepare_cached(&format!("SELECT 1 FROM entries WHERE parent_id = ?1 AND name_lower GLOB ?2 AND id != 0 {kind_clause} LIMIT 1"))?
                    .query_row(params![parent, p], |r| r.get(0))
                    .optional()?
            } else {
                self.conn
                    .prepare_cached(&format!("SELECT 1 FROM entries WHERE parent_id = ?1 AND name_lower = ?2 AND id != 0 {kind_clause} LIMIT 1"))?
                    .query_row(params![parent, p], |r| r.get(0))
                    .optional()?
            };
            if found.is_some() {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn extracted_sibling(&self, row: &Row) -> rusqlite::Result<bool> {
        if row.is_dir() || row.size <= 0 {
            return Ok(false);
        }
        let lower = row.name.to_lowercase();
        let Some((stem, ext)) = lower.rsplit_once('.') else { return Ok(false) };
        if !ARCHIVE_EXTS.contains(&ext) {
            return Ok(false);
        }
        let stem = stem.strip_suffix(".tar").unwrap_or(stem);
        let Some(parent) = row.parent_id else { return Ok(false) };
        let dir_size: Option<i64> = self
            .conn
            .prepare_cached("SELECT size FROM entries WHERE parent_id = ?1 AND name_lower = ?2 AND kind = 1 LIMIT 1")?
            .query_row(params![parent, stem], |r| r.get(0))
            .optional()?;
        Ok(dir_size.is_some_and(|d| d.saturating_mul(2) >= row.size))
    }

    /// Conditions and age. All answered from snapshot rows.
    fn passes(&self, rule: &RuleDef, row: &Row) -> rusqlite::Result<bool> {
        let c: &Conditions = &rule.conditions;
        if let Some(min) = c.min_size {
            if (row.alloc.max(0) as u64) < min.0 {
                return Ok(false);
            }
        }
        if let Some(days) = rule.min_age_days {
            // Last-modified time; folders use their newest file. Unknown age never matches.
            let ts = if row.is_dir() { row.max_mtime.or(row.mtime) } else { row.mtime };
            let Some(ts) = ts.filter(|t| *t > 0) else { return Ok(false) };
            if self.opts.now - ts < i64::from(days) * 86_400 {
                return Ok(false);
            }
        }
        if let Some(s) = &c.sibling_exists {
            let Some(parent) = row.parent_id else { return Ok(false) };
            if !self.named_child_exists(parent, &s.to_vec(), false)? {
                return Ok(false);
            }
        }
        if let Some(s) = &c.child_exists {
            if !row.is_dir() || !self.named_child_exists(row.id, &s.to_vec(), false)? {
                return Ok(false);
            }
        }
        if c.extracted_sibling && !self.extracted_sibling(row)? {
            return Ok(false);
        }
        Ok(true)
    }

    fn applies(rule: &RuleDef, row: &Row) -> bool {
        match rule.applies_to {
            Some(AppliesTo::Dir) => row.is_dir(),
            Some(AppliesTo::File) => !row.is_dir(),
            None => true,
        }
    }

    fn accept(&self, rule: &RuleDef, row: Row, path: String) -> rusqlite::Result<Option<Cand>> {
        if row.kind == 2 || row.cloud_placeholder() || !Self::applies(rule, &row) {
            return Ok(None);
        }
        if rule.kind == RuleKind::ManualCleanup && self.protected.is_protected(&path) {
            return Ok(None);
        }
        if !self.passes(rule, &row)? {
            return Ok(None);
        }
        Ok(Some(Cand::from_row(row, path)))
    }

    // -- one rule ---------------------------------------------------------

    fn run_rule(&self, rule: &RuleDef) -> Result<Vec<Cand>, String> {
        if let Some(v) = rule.virtual_source {
            return Ok(self.virtual_cands(rule, v));
        }
        let mut found: Vec<Cand> = Vec::new();
        let err = |e: rusqlite::Error| e.to_string();

        // Scopes: each expanded path inside this snapshot, or the whole snapshot.
        let mut scopes: Vec<(Row, String)> = Vec::new();
        if rule.paths.is_empty() {
            if let Some(root) = self.row_by_id(0).map_err(err)? {
                scopes.push((root, self.root.clone()));
            }
        }
        for template in &rule.paths {
            let Ok(abs) = expand(template, self.opts.env) else { continue };
            if self.rel_to_root(&abs).is_none() {
                continue;
            }
            match self.resolve(&abs).map_err(err)? {
                Some(row) => scopes.push((row, abs)),
                None => {
                    if rule.globs.is_empty() {
                        if let Some(c) = self.fallback_file(rule, &abs) {
                            found.push(c);
                        }
                    }
                }
            }
        }

        for (scope, base) in scopes {
            if rule.globs.is_empty() {
                let path = base.clone();
                if let Some(c) = self.accept(rule, scope, path).map_err(err)? {
                    found.push(c);
                }
                continue;
            }
            if !scope.is_dir() {
                continue;
            }
            for g in &rule.globs {
                found.extend(self.glob_in(rule, &scope, &base, g)?);
            }
        }
        Ok(dedupe_nested(found))
    }

    fn glob_in(&self, rule: &RuleDef, scope: &Row, base: &str, glob: &str) -> Result<Vec<Cand>, String> {
        let err = |e: rusqlite::Error| e.to_string();
        let plan = plan(glob)?;
        let (lo, hi) = (scope.id + 1, scope.last_desc);
        if lo > hi {
            return Ok(Vec::new());
        }
        let (clause, arg): (&str, Option<String>) = match &plan.name {
            NameFilter::Literal(n) => ("AND name_lower = ?3", Some(n.clone())),
            NameFilter::Prefix(p) => ("AND name_lower GLOB ?3", Some(format!("{}*", glob_escape(p)))),
            NameFilter::Suffix(s) => ("AND name_lower GLOB ?3", Some(format!("*{}", glob_escape(s)))),
            NameFilter::Other => ("AND ?3 IS NULL", None),
        };
        let sql =
            format!("SELECT {COLS} FROM entries WHERE id BETWEEN ?1 AND ?2 AND kind IN (0, 1) {clause} ORDER BY id");
        let rows: Vec<Row> = {
            let mut stmt = self.conn.prepare_cached(&sql).map_err(err)?;
            let rows = stmt.query_map(params![lo, hi, arg], row_from).map_err(err)?;
            rows.collect::<Result<_, _>>().map_err(err)?
        };
        let base_lower = base.trim_end_matches('\\').to_lowercase();
        let mut out = Vec::new();
        let mut skip_until = i64::MIN;
        for row in rows {
            if row.id <= skip_until {
                continue;
            }
            if matches!(plan.name, NameFilter::Other) && !plan.name_matcher.is_match(&row.name) {
                continue;
            }
            let path = self.path_of(&row).map_err(err)?;
            let Some(rel) = within_rel(&path, &base_lower) else { continue };
            if !plan.full.is_match(rel.replace('\\', "/")) {
                continue;
            }
            let (is_dir, last) = (row.is_dir(), row.last_desc);
            if let Some(c) = self.accept(rule, row, path).map_err(err)? {
                if is_dir {
                    skip_until = last;
                }
                out.push(c);
            }
        }
        Ok(out)
    }

    /// A well-known system file that is not in the snapshot (for example the
    /// scan skipped it): ask for its attributes only.
    fn fallback_file(&self, rule: &RuleDef, abs: &str) -> Option<Cand> {
        if rule.kind != RuleKind::SystemSetting || !self.drive_root {
            return None;
        }
        match self.opts.probe.file_size(abs) {
            Ok(n) => Some(Cand::virtual_item("file", abs.to_string(), Some(n), None)),
            Err(ProbeError::Missing) => None,
            Err(e) => {
                tracing::warn!(path = abs, error = %e, "cannot read the size of a system file");
                Some(Cand::virtual_item("file", abs.to_string(), None, Some(format!("Size unknown: {e}"))))
            }
        }
    }

    fn virtual_cands(&self, rule: &RuleDef, source: VirtualSource) -> Vec<Cand> {
        if !self.drive_root {
            return Vec::new();
        }
        let letter = self.root.chars().next().unwrap_or('?').to_ascii_uppercase();
        match source {
            VirtualSource::RecycleBin => match self.opts.probe.recycle_bin(&self.root) {
                Ok(rb) if rb.bytes > 0 => {
                    let noun = if rb.items == 1 { "item" } else { "items" };
                    let note = Some(format!("{} {noun} in your Recycle Bin.", rb.items));
                    vec![Cand::virtual_item("recycle-bin", format!("Recycle Bin ({letter}:)"), Some(rb.bytes), note)]
                }
                Ok(_) | Err(ProbeError::Missing) => Vec::new(),
                Err(e) => {
                    tracing::warn!(rule = %rule.id, error = %e, "cannot measure the Recycle Bin");
                    vec![Cand::virtual_item(
                        "recycle-bin",
                        format!("Recycle Bin ({letter}:)"),
                        None,
                        Some(format!("Size unknown: {e}")),
                    )]
                }
            },
            VirtualSource::ShadowStorage => {
                if self.system_drive.as_deref().map(str::to_uppercase) != Some(self.root.to_uppercase()) {
                    return Vec::new();
                }
                let label = format!("Shadow copies ({letter}:)");
                match self.opts.probe.shadow_storage(&self.root) {
                    Ok(bytes) => vec![Cand::virtual_item("shadow-storage", label, Some(bytes), None)],
                    Err(ProbeError::Missing) => Vec::new(),
                    Err(e) => vec![Cand::virtual_item("shadow-storage", label, None, Some(e.to_string()))],
                }
            }
        }
    }
}

fn diskdash_path_join(dir: &str, name: &str) -> String {
    if dir.ends_with('\\') {
        format!("{dir}{name}")
    } else {
        format!("{dir}\\{name}")
    }
}

/// Within one rule keep only the outermost of nested matches (and drop repeats).
fn dedupe_nested(mut cands: Vec<Cand>) -> Vec<Cand> {
    cands.sort_by_key(|c| c.row.as_ref().map_or(i64::MAX, |r| r.id));
    let mut out: Vec<Cand> = Vec::with_capacity(cands.len());
    let mut end = i64::MIN;
    for c in cands {
        match &c.row {
            Some(r) => {
                if r.id <= end {
                    continue;
                }
                end = r.last_desc;
                out.push(c);
            }
            None => out.push(c),
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Evaluation
// ---------------------------------------------------------------------------

struct Found {
    rule_pos: usize,
    cand: Cand,
}

/// Evaluate `rules` (already filtered to the active ones) against the snapshot
/// in `conn` and store the result in it, replacing any earlier result.
///
/// `progress(done, total, rule_title)` is called after every rule; returning
/// `false` cancels the run (nothing is written).
pub fn evaluate(
    conn: &mut Connection,
    rules: &[&LoadedRule],
    opts: &EvalOptions<'_>,
    progress: &mut dyn FnMut(usize, usize, &str) -> bool,
) -> Result<EvalSummary, EvalError> {
    conn.execute_batch(SCHEMA)?;
    conn.execute_batch("CREATE INDEX IF NOT EXISTS entries_name_lower ON entries(name_lower);")?;

    let mut summary = EvalSummary { rules_evaluated: rules.len(), ..EvalSummary::default() };
    let mut found: Vec<Found> = Vec::new();
    {
        let ctx = Ctx::new(conn, opts)?;
        for (pos, rule) in rules.iter().enumerate() {
            if !progress(pos, rules.len(), &rule.def.title) {
                return Err(EvalError::Cancelled);
            }
            match ctx.run_rule(&rule.def) {
                Ok(cands) => found.extend(cands.into_iter().map(|cand| Found { rule_pos: pos, cand })),
                Err(message) => {
                    tracing::warn!(rule = %rule.def.id, %message, "rule evaluation failed");
                    summary.rule_failures.push(RuleFailure { rule_id: rule.def.id.clone(), message });
                }
            }
        }
        progress(rules.len(), rules.len(), "Saving results");
    }

    // Outermost first; equal entries ordered by rule position (deterministic).
    found.sort_by(|a, b| {
        let key = |f: &Found| (f.cand.row.as_ref().map_or(i64::MAX, |r| r.id), f.rule_pos, f.cand.path.clone());
        key(a).cmp(&key(b))
    });

    let tx = conn.transaction()?;
    tx.execute_batch("DELETE FROM rule_matches; DELETE FROM rule_defs;")?;
    {
        let mut def_stmt = tx.prepare(
            "INSERT INTO rule_defs (rule_id, title, category, explanation, risk, kind, instructions) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        )?;
        for r in rules {
            def_stmt.execute(params![
                r.def.id,
                r.def.title,
                r.def.category,
                r.def.explanation,
                r.def.risk.as_str(),
                r.def.kind.as_str(),
                serde_json::to_string(&r.def.instructions).unwrap_or_else(|_| "[]".into()),
            ])?;
        }
        let mut ins = tx.prepare(
            "INSERT INTO rule_matches (rule_id, entry_id, virtual_key, path, is_dir, logical, allocated, file_count, modified,
                                       risk, kind, category, counted, parent_match, note)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
        )?;
        // The outermost match so far: (row id of the match, last id of its subtree).
        let mut outer: Option<(i64, i64)> = None;
        let mut totals: HashMap<(Risk, RuleKind), Saving> = HashMap::new();
        for f in &found {
            let rule = &rules[f.rule_pos].def;
            let (entry_id, last) = match &f.cand.row {
                Some(r) => (Some(r.id), r.last_desc),
                None => (None, i64::MAX),
            };
            let nested = match (entry_id, outer) {
                (Some(id), Some((_, end))) => id <= end,
                _ => false,
            };
            let parent = if nested { outer.map(|(m, _)| m) } else { None };
            ins.execute(params![
                rule.id,
                entry_id,
                f.cand.virtual_key,
                f.cand.path,
                f.cand.is_dir,
                f.cand.logical.map(|v| v as i64),
                f.cand.allocated.map(|v| v as i64),
                f.cand.file_count.map(|v| v as i64),
                f.cand.modified,
                rule.risk.as_str(),
                rule.kind.as_str(),
                rule.category,
                !nested,
                parent,
                f.cand.note,
            ])?;
            let match_id = tx.last_insert_rowid();
            summary.matches += 1;
            if nested {
                continue;
            }
            if entry_id.is_some() {
                outer = Some((match_id, last));
            }
            summary.counted += 1;
            let s = totals.entry((rule.risk, rule.kind)).or_insert(Saving {
                risk: rule.risk,
                kind: rule.kind,
                allocated: 0,
                logical: 0,
                matches: 0,
                unknown: 0,
            });
            s.matches += 1;
            match f.cand.allocated {
                Some(a) => {
                    s.allocated += a;
                    s.logical += f.cand.logical.unwrap_or(a);
                }
                None => s.unknown += 1,
            }
        }
        let mut savings: Vec<Saving> = totals.into_values().collect();
        savings.sort_by_key(|s| (s.risk, s.kind.as_str()));
        summary.savings = savings;
    }
    tx.commit()?;
    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_filters() {
        assert_eq!(name_filter("node_modules"), NameFilter::Literal("node_modules".into()));
        assert_eq!(name_filter("*.LOG"), NameFilter::Suffix(".log".into()));
        assert_eq!(name_filter("~$*"), NameFilter::Prefix("~$".into()));
        assert_eq!(name_filter("a*b"), NameFilter::Other);
        assert_eq!(name_filter("*.{bin,obj}"), NameFilter::Other);
    }

    #[test]
    fn glob_escape_is_sqlite_safe() {
        assert_eq!(glob_escape("a*b?[c"), "a[*]b[?][[]c");
    }

    #[test]
    fn globs_without_slash_match_any_depth_and_ignore_case() {
        let p = plan("node_modules").unwrap();
        assert!(p.full.is_match("node_modules"));
        assert!(p.full.is_match("a/b/NODE_MODULES"));
        assert!(!p.full.is_match("a/node_modules_x"));
        let p = plan("Cache/*/Cache_Data").unwrap();
        assert!(p.full.is_match("cache/Default/cache_data"));
        assert!(!p.full.is_match("x/cache/Default/cache_data"));
    }

    #[test]
    fn relative_paths_are_case_insensitive() {
        assert_eq!(within_rel(r"C:\Users\Ann", "c:"), Some(r"Users\Ann"));
        assert_eq!(within_rel(r"D:\Projects\app", r"d:\projects"), Some("app"));
        assert_eq!(within_rel(r"D:\Projects", r"d:\projects"), Some(""));
        assert_eq!(within_rel(r"D:\ProjectsX", r"d:\projects"), None);
    }
}

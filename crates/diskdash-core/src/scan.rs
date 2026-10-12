//! The directory-walk scanner.
//!
//! The root's files form work unit 0; each subdirectory of the root is a
//! further unit with its own id block. Units run on `threads` worker threads
//! (1 for an HDD), each doing an iterative DFS with post-order aggregation, so
//! memory is bounded by depth plus batch size plus the hard-link map.

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use crossbeam_channel::Sender;
use rusqlite::{params, Connection, OptionalExtension};

use crate::alloc;
use crate::category::{self, SystemPaths};
use crate::paths::{self, to_extended};
use crate::reparse::{self, Action};
use crate::store::{
    self, DirTotals, ErrRow, Msg, Row, Writer, BATCH_ROWS, FLAG_ACCESS_DENIED, FLAG_CLOUD_PLACEHOLDER,
    FLAG_LINK_COUNT_UNKNOWN, KIND_DIR, KIND_FILE, KIND_LINK,
};
use crate::win::{self, DirIter, FindEntry, MetaHandle, WinError, ERROR_ACCESS_DENIED};

const BLOCK_SHIFT: u32 = 40;
const CANCEL_CHECK_EVERY: u32 = 256;

pub fn block_range(block: i64) -> (i64, i64) {
    let start = block << BLOCK_SHIFT;
    (start, start | ((1i64 << BLOCK_SHIFT) - 1))
}

#[derive(Debug, Clone)]
pub struct ScanOptions {
    pub root: PathBuf,
    pub db_path: PathBuf,
    /// Traversal threads for this root (1 on an HDD).
    pub threads: usize,
    /// Open a metadata handle per file to count hard links once.
    pub detect_hardlinks: bool,
    /// Directories never entered (e.g. diskdash's own data directory).
    pub exclude: Vec<String>,
    /// Continue a cancelled/interrupted snapshot in `db_path`.
    pub resume: bool,
    /// `%WINDIR%`, for the system category override.
    pub windows_dir: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum ScanError {
    #[error("cannot read the scan root {root}: {source}")]
    RootUnreadable { root: String, source: WinError },
    #[error("cannot read volume information for {root}: {source}")]
    Volume { root: String, source: WinError },
    #[error("invalid scan root: {0}")]
    BadRoot(String),
    #[error("snapshot database error: {0}")]
    Db(String),
    #[error("the volume changed since this scan started (serial {expected:08X} → {found:08X}); start a new scan")]
    SerialMismatch { expected: u32, found: u32 },
}

impl From<rusqlite::Error> for ScanError {
    fn from(e: rusqlite::Error) -> Self {
        ScanError::Db(e.to_string())
    }
}

#[derive(Debug, Default)]
pub struct Counters {
    pub files: AtomicU64,
    pub dirs: AtomicU64,
    pub bytes: AtomicU64,
    pub errors: AtomicU64,
    pub units_done: AtomicU64,
    pub units_total: AtomicU64,
    current: Mutex<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScanProgress {
    pub files: u64,
    pub dirs: u64,
    pub bytes: u64,
    pub errors: u64,
    pub units_done: u64,
    pub units_total: u64,
    pub current_dir: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanStatus {
    Complete,
    Partial,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScanStats {
    pub files: u64,
    pub dirs: u64,
    pub links: u64,
    pub size: u64,
    pub alloc: u64,
    pub errors: u64,
    pub access_denied: u64,
    pub link_count_unknown: u64,
}

#[derive(Debug, Clone)]
pub struct ScanSummary {
    pub status: ScanStatus,
    pub root: String,
    pub volume_serial: u32,
    pub filesystem: String,
    pub cluster_size: u64,
    pub hardlinks_deduplicated: bool,
    pub stats: ScanStats,
}

/// A running scan.
pub struct ScanHandle {
    cancel: Arc<AtomicBool>,
    counters: Arc<Counters>,
    thread: Option<JoinHandle<Result<ScanSummary, ScanError>>>,
}

impl ScanHandle {
    pub fn progress(&self) -> ScanProgress {
        let c = &self.counters;
        ScanProgress {
            files: c.files.load(Ordering::Relaxed),
            dirs: c.dirs.load(Ordering::Relaxed),
            bytes: c.bytes.load(Ordering::Relaxed),
            errors: c.errors.load(Ordering::Relaxed),
            units_done: c.units_done.load(Ordering::Relaxed),
            units_total: c.units_total.load(Ordering::Relaxed),
            current_dir: c.current.lock().map(|s| s.clone()).unwrap_or_default(),
        }
    }

    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    pub fn is_finished(&self) -> bool {
        self.thread.as_ref().is_none_or(|t| t.is_finished())
    }

    pub fn join(mut self) -> Result<ScanSummary, ScanError> {
        match self.thread.take() {
            Some(t) => t.join().unwrap_or_else(|_| Err(ScanError::Db("scan thread panicked".into()))),
            None => Err(ScanError::Db("already joined".into())),
        }
    }
}

/// Start scanning on a background thread.
pub fn scan(opts: ScanOptions) -> Result<ScanHandle, ScanError> {
    let cancel = Arc::new(AtomicBool::new(false));
    let counters = Arc::new(Counters::default());
    let (c2, k2) = (cancel.clone(), counters.clone());
    let thread = std::thread::Builder::new()
        .name("scan".into())
        .spawn(move || run(opts, c2, k2))
        .map_err(|e| ScanError::Db(e.to_string()))?;
    Ok(ScanHandle { cancel, counters, thread: Some(thread) })
}

/// Scan synchronously (tests, benchmarks).
pub fn scan_blocking(opts: ScanOptions) -> Result<ScanSummary, ScanError> {
    scan(opts)?.join()
}

struct Ctx {
    root: String,
    cluster: u64,
    detect_hardlinks: bool,
    exclude: Vec<String>,
    system: SystemPaths,
    cancel: Arc<AtomicBool>,
    counters: Arc<Counters>,
    hardlinks: Mutex<HashMap<u128, i64>>,
    tx: Sender<Msg>,
}

struct Cancelled;

fn is_dir(attrs: u32) -> bool {
    attrs & alloc::FILE_ATTRIBUTE_DIRECTORY != 0
}

impl Ctx {
    fn excluded(&self, path: &str) -> bool {
        self.exclude.iter().any(|x| paths::is_within(path, x))
    }

    fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    fn link_row(&self, id: i64, parent: i64, depth: i64, dir_path: &str, e: &FindEntry) -> Row {
        let target =
            MetaHandle::open(&to_extended(&paths::join(dir_path, &e.name))).ok().and_then(|h| h.reparse_target());
        Row {
            id,
            parent_id: Some(parent),
            name: e.name.clone(),
            kind: KIND_LINK,
            depth,
            ctime: e.ctime,
            mtime: e.mtime,
            atime: e.atime,
            attrs: e.attrs,
            reparse_tag: e.reparse_tag,
            link_target: target,
            ..Row::default()
        }
    }

    /// Build a file row; returns the row and whether it counts toward totals
    /// (false for later paths of a hard-linked file).
    fn file_row(&self, id: i64, parent: i64, depth: i64, dir_path: &str, e: &FindEntry) -> (Row, bool) {
        let ext = category::extension(&e.name);
        let cat = category::categorize(&self.system, dir_path, &e.name, &ext);
        let placeholder = alloc::is_cloud_placeholder(e.attrs);
        let mut flags = if placeholder { FLAG_CLOUD_PLACEHOLDER } else { 0 };
        let (mut nlink, mut file_id, mut hardlink_of, mut queried) = (None, None, None, None);
        let ext_path = to_extended(&paths::join(dir_path, &e.name));

        if self.detect_hardlinks && !alloc::must_not_open(e.attrs) {
            match MetaHandle::open(&ext_path).and_then(|h| h.standard_info().map(|si| (h, si))) {
                Ok((h, si)) => {
                    nlink = Some(si.links);
                    if !placeholder && !alloc::needs_query(e.attrs, e.reparse_tag) {
                        queried = Some(si.allocation);
                    }
                    if si.links > 1 {
                        if let Ok((_, fid)) = h.file_id() {
                            file_id = Some(fid);
                            if let Ok(mut map) = self.hardlinks.lock() {
                                match map.get(&fid) {
                                    Some(first) => hardlink_of = Some(*first),
                                    None => {
                                        map.insert(fid, id);
                                    }
                                }
                            }
                        }
                    }
                }
                Err(_) => flags |= FLAG_LINK_COUNT_UNKNOWN,
            }
        }
        if queried.is_none() && !placeholder && alloc::needs_query(e.attrs, e.reparse_tag) {
            queried = win::compressed_size(&ext_path);
        }
        let allocated = alloc::allocated(e.size, e.attrs, self.cluster, queried);
        let counts = hardlink_of.is_none();
        if counts {
            self.counters.files.fetch_add(1, Ordering::Relaxed);
            self.counters.bytes.fetch_add(e.size, Ordering::Relaxed);
        }
        let row = Row {
            id,
            parent_id: Some(parent),
            name: e.name.clone(),
            kind: KIND_FILE,
            depth,
            size: e.size,
            alloc: allocated,
            ctime: e.ctime,
            mtime: e.mtime,
            atime: e.atime,
            attrs: e.attrs,
            reparse_tag: e.reparse_tag,
            file_id,
            nlink,
            hardlink_of,
            ext: Some(ext),
            category: Some(cat as u8),
            flags,
            ..Row::default()
        };
        (row, counts)
    }
}

/// Totals contributed by one file row.
fn add_file(t: &mut DirTotals, r: &Row, counts: bool) {
    if counts {
        t.size += r.size;
        t.alloc += r.alloc;
        t.files += 1;
        t.max_mtime = t.max_mtime.max(r.mtime);
        t.max_atime = t.max_atime.max(r.atime);
        if let Some(cat) = r.category {
            t.cats[usize::from(cat).min(9)] += r.alloc;
        }
    }
}

/// A child directory's totals folded into its parent.
fn add_dir(parent: &mut DirTotals, child: &DirTotals) {
    parent.size += child.size;
    parent.alloc += child.alloc;
    parent.files += child.files;
    parent.dirs += 1 + child.dirs;
    parent.max_mtime = parent.max_mtime.max(child.max_mtime);
    parent.max_atime = parent.max_atime.max(child.max_atime);
    for (a, b) in parent.cats.iter_mut().zip(child.cats.iter()) {
        *a += b;
    }
}

struct Batch<'a> {
    ctx: &'a Ctx,
    block: i64,
    rows: Vec<Row>,
    errors: Vec<ErrRow>,
}

impl Batch<'_> {
    fn row(&mut self, r: Row) {
        self.rows.push(r);
        if self.rows.len() >= BATCH_ROWS {
            let _ = self.ctx.tx.send(Msg::Rows(std::mem::take(&mut self.rows)));
        }
    }

    fn error(&mut self, path: &str, op: &'static str, e: &WinError) {
        self.ctx.counters.errors.fetch_add(1, Ordering::Relaxed);
        self.errors.push(ErrRow {
            block: self.block,
            path: path.to_string(),
            op,
            code: e.code,
            message: e.message.clone(),
        });
    }

    fn flush(&mut self) {
        if !self.rows.is_empty() {
            let _ = self.ctx.tx.send(Msg::Rows(std::mem::take(&mut self.rows)));
        }
        if !self.errors.is_empty() {
            let _ = self.ctx.tx.send(Msg::Errors(std::mem::take(&mut self.errors)));
        }
    }
}

struct Frame {
    id: i64,
    parent_id: i64,
    path: String,
    depth: i64,
    meta: FindEntry,
    iter: DirIter,
    totals: DirTotals,
}

/// Walk one root subdirectory (a work unit). Returns the top directory's
/// totals (its descendants), with `last_desc_id` set.
fn walk_unit(ctx: &Ctx, block: i64, top: &FindEntry) -> Result<DirTotals, Cancelled> {
    let mut seq: i64 = 0;
    let mut next_id = || {
        let id = (block << BLOCK_SHIFT) | seq;
        seq += 1;
        id
    };
    let mut batch = Batch { ctx, block, rows: Vec::new(), errors: Vec::new() };
    let top_path = paths::join(&ctx.root, &top.name);
    let top_id = next_id();
    ctx.counters.dirs.fetch_add(1, Ordering::Relaxed);

    let iter = match DirIter::open(&to_extended(&top_path)) {
        Ok(it) => it,
        Err(e) => {
            let t = DirTotals { last_desc_id: top_id, ..DirTotals::default() };
            batch.error(&top_path, "list", &e);
            batch.row(dir_row(top_id, 0, 1, top, &top_path, &t, denied_flag(&e)));
            batch.flush();
            return Ok(t);
        }
    };
    let mut stack = vec![Frame {
        id: top_id,
        parent_id: 0,
        path: top_path,
        depth: 1,
        meta: top.clone(),
        iter,
        totals: DirTotals::default(),
    }];
    let mut since_check = 0u32;
    let mut last_id = top_id;

    loop {
        since_check += 1;
        if since_check >= CANCEL_CHECK_EVERY {
            since_check = 0;
            if ctx.cancelled() {
                batch.flush();
                return Err(Cancelled);
            }
        }
        let Some(frame) = stack.last_mut() else { break };
        let next = frame.iter.next();
        match next {
            None => {
                let f = stack.pop().expect("frame");
                let mut t = f.totals;
                t.last_desc_id = last_id;
                batch.row(dir_row(f.id, f.parent_id, f.depth, &f.meta, &f.path, &t, 0));
                match stack.last_mut() {
                    Some(parent) => add_dir(&mut parent.totals, &t),
                    None => {
                        batch.flush();
                        return Ok(t);
                    }
                }
            }
            Some(Err(e)) => {
                let path = frame.path.clone();
                batch.error(&path, "list", &e);
            }
            Some(Ok(e)) => {
                let (parent_id, depth, dir_path) = (frame.id, frame.depth + 1, frame.path.clone());
                let link = reparse::action(e.reparse_tag, is_dir(e.attrs)) == Action::Link && e.reparse_tag != 0;
                if link {
                    let id = next_id();
                    last_id = id;
                    batch.row(ctx.link_row(id, parent_id, depth, &dir_path, &e));
                    continue;
                }
                if is_dir(e.attrs) {
                    let child = paths::join(&dir_path, &e.name);
                    if ctx.excluded(&child) {
                        continue;
                    }
                    let id = next_id();
                    last_id = id;
                    ctx.counters.dirs.fetch_add(1, Ordering::Relaxed);
                    if let Ok(mut cur) = ctx.counters.current.lock() {
                        cur.clone_from(&child);
                    }
                    match DirIter::open(&to_extended(&child)) {
                        Ok(iter) => stack.push(Frame {
                            id,
                            parent_id,
                            path: child,
                            depth,
                            meta: e,
                            iter,
                            totals: DirTotals::default(),
                        }),
                        Err(err) => {
                            batch.error(&child, "list", &err);
                            let t = DirTotals { last_desc_id: id, ..DirTotals::default() };
                            batch.row(dir_row(id, parent_id, depth, &e, &child, &t, denied_flag(&err)));
                            if let Some(parent) = stack.last_mut() {
                                add_dir(&mut parent.totals, &t);
                            }
                        }
                    }
                } else {
                    let id = next_id();
                    last_id = id;
                    let (row, counts) = ctx.file_row(id, parent_id, depth, &dir_path, &e);
                    if let Some(f) = stack.last_mut() {
                        add_file(&mut f.totals, &row, counts);
                    }
                    batch.row(row);
                }
            }
        }
    }
    batch.flush();
    Ok(DirTotals::default())
}

fn denied_flag(e: &WinError) -> i64 {
    if e.code == ERROR_ACCESS_DENIED {
        FLAG_ACCESS_DENIED
    } else {
        0
    }
}

fn dir_row(id: i64, parent: i64, depth: i64, e: &FindEntry, path: &str, t: &DirTotals, flags: i64) -> Row {
    Row {
        id,
        parent_id: Some(parent),
        name: e.name.clone(),
        kind: KIND_DIR,
        depth,
        size: t.size,
        alloc: t.alloc,
        ctime: e.ctime,
        mtime: e.mtime,
        atime: e.atime,
        attrs: e.attrs,
        reparse_tag: e.reparse_tag,
        flags,
        path: Some(path.to_string()),
        dir: Some(*t),
        ..Row::default()
    }
}

fn meta_get(conn: &Connection, key: &str) -> rusqlite::Result<Option<String>> {
    conn.query_row("SELECT value FROM meta WHERE key = ?1", [key], |r| r.get(0)).optional()
}

fn meta_set(conn: &Connection, key: &str, value: &str) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO meta (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )?;
    Ok(())
}

fn run(opts: ScanOptions, cancel: Arc<AtomicBool>, counters: Arc<Counters>) -> Result<ScanSummary, ScanError> {
    let root = paths::normalize_root(&opts.root).map_err(|e| ScanError::BadRoot(e.to_string()))?;
    let vol = win::volume_info(&root).map_err(|source| ScanError::Volume { root: root.clone(), source })?;

    // List the root first: if that fails, nothing is written as complete.
    let root_entries: Vec<FindEntry> = DirIter::open(&to_extended(&root))
        .map_err(|source| ScanError::RootUnreadable { root: root.clone(), source })?
        .filter_map(Result::ok)
        .collect();

    let conn = store::open(&opts.db_path)?;
    let mut hardlinks = HashMap::new();
    if opts.resume {
        let expected: u32 = meta_get(&conn, "volume_serial")?.and_then(|s| s.parse().ok()).unwrap_or(vol.serial);
        if expected != vol.serial {
            return Err(ScanError::SerialMismatch { expected, found: vol.serial });
        }
        let pending: Vec<i64> = conn
            .prepare("SELECT block FROM units WHERE status != 'done'")?
            .query_map([], |r| r.get(0))?
            .collect::<Result<_, _>>()?;
        for b in &pending {
            let (lo, hi) = block_range(*b);
            conn.execute("DELETE FROM entries WHERE id BETWEEN ?1 AND ?2", params![lo.max(1), hi])?;
            conn.execute("DELETE FROM errors WHERE block = ?1", [b])?;
        }
        conn.execute("DELETE FROM entries WHERE id = 0", [])?;
        let mut stmt =
            conn.prepare("SELECT file_id, id FROM entries WHERE file_id IS NOT NULL AND hardlink_of IS NULL")?;
        for row in stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))? {
            let (fid, id) = row?;
            if let Ok(fid) = u128::from_str_radix(&fid, 16) {
                hardlinks.insert(fid, id);
            }
        }
        meta_set(&conn, "resumed", "1")?;
    } else {
        meta_set(&conn, "root", &root)?;
        meta_set(&conn, "volume_serial", &vol.serial.to_string())?;
        meta_set(&conn, "filesystem", &vol.filesystem)?;
        meta_set(&conn, "cluster_size", &vol.cluster_size.to_string())?;
        meta_set(&conn, "hardlinks", if opts.detect_hardlinks { "detected" } else { "not_deduplicated" })?;
        conn.execute("INSERT OR REPLACE INTO units (block, path, status) VALUES (0, ?1, 'pending')", [&root])?;
        let mut block = 1i64;
        for e in &root_entries {
            let child = paths::join(&root, &e.name);
            let enter = is_dir(e.attrs)
                && !(reparse::action(e.reparse_tag, true) == Action::Link && e.reparse_tag != 0)
                && !opts.exclude.iter().any(|x| paths::is_within(&child, x));
            if enter {
                conn.execute(
                    "INSERT INTO units (block, path, status) VALUES (?1, ?2, 'pending')",
                    params![block, child],
                )?;
                block += 1;
            }
        }
    }
    meta_set(&conn, "status", "running")?;

    let units: Vec<(i64, String, String)> = conn
        .prepare("SELECT block, path, status FROM units ORDER BY block")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect::<Result<_, _>>()?;
    counters.units_total.store(units.len() as u64, Ordering::Relaxed);
    counters.units_done.store(units.iter().filter(|u| u.2 == "done").count() as u64, Ordering::Relaxed);

    let writer = Writer::start(conn, 8);
    let ctx = Ctx {
        root: root.clone(),
        cluster: vol.cluster_size,
        detect_hardlinks: opts.detect_hardlinks,
        exclude: opts.exclude.clone(),
        system: opts.windows_dir.as_deref().map(SystemPaths::new).unwrap_or_else(SystemPaths::from_env),
        cancel: cancel.clone(),
        counters: counters.clone(),
        hardlinks: Mutex::new(hardlinks),
        tx: writer.sender(),
    };

    // Unit 0: the root's own files and links.
    if units.iter().any(|u| u.0 == 0 && u.2 != "done") {
        let mut batch = Batch { ctx: &ctx, block: 0, rows: Vec::new(), errors: Vec::new() };
        let mut totals = DirTotals::default();
        let mut seq = 1i64;
        for e in &root_entries {
            let link = reparse::action(e.reparse_tag, is_dir(e.attrs)) == Action::Link && e.reparse_tag != 0;
            if link {
                batch.row(ctx.link_row(seq, 0, 1, &root, e));
                seq += 1;
            } else if !is_dir(e.attrs) {
                let (row, counts) = ctx.file_row(seq, 0, 1, &root, e);
                add_file(&mut totals, &row, counts);
                batch.row(row);
                seq += 1;
            }
        }
        totals.last_desc_id = seq - 1;
        batch.flush();
        let _ = ctx.tx.send(Msg::UnitDone { block: 0, totals });
        counters.units_done.fetch_add(1, Ordering::Relaxed);
    }

    // Units 1..: root subdirectories, on `threads` workers.
    let by_name: HashMap<String, &FindEntry> =
        root_entries.iter().map(|e| (paths::join(&root, &e.name).to_lowercase(), e)).collect();
    let queue: Mutex<VecDeque<(i64, FindEntry)>> = Mutex::new(
        units
            .iter()
            .filter(|u| u.0 != 0 && u.2 != "done")
            .filter_map(|u| by_name.get(&u.1.to_lowercase()).map(|e| (u.0, (*e).clone())))
            .collect(),
    );
    let vanished: Vec<i64> = units
        .iter()
        .filter(|u| u.0 != 0 && u.2 != "done" && !by_name.contains_key(&u.1.to_lowercase()))
        .map(|u| u.0)
        .collect();
    for b in vanished {
        // Deleted since the scan started: an empty, finished unit.
        let _ = ctx.tx.send(Msg::UnitDone { block: b, totals: DirTotals::default() });
        counters.units_done.fetch_add(1, Ordering::Relaxed);
    }

    let threads = opts.threads.max(1);
    std::thread::scope(|s| {
        for _ in 0..threads {
            s.spawn(|| loop {
                if ctx.cancelled() {
                    return;
                }
                let Some((block, entry)) = queue.lock().ok().and_then(|mut q| q.pop_front()) else { return };
                if let Ok(t) = walk_unit(&ctx, block, &entry) {
                    let _ = ctx.tx.send(Msg::UnitDone { block, totals: t });
                    ctx.counters.units_done.fetch_add(1, Ordering::Relaxed);
                }
            });
        }
    });

    let Ctx { tx, .. } = ctx;
    drop(tx);
    let mut conn = writer.finish().map_err(ScanError::Db)?;
    let cancelled = cancel.load(Ordering::Relaxed);
    let pending: i64 = conn.query_row("SELECT count(*) FROM units WHERE status != 'done'", [], |r| r.get(0))?;
    let status = if cancelled || pending > 0 { ScanStatus::Partial } else { ScanStatus::Complete };

    if status == ScanStatus::Complete {
        write_root(&mut conn, &root)?;
        store::finalize(&conn)?;
    }
    meta_set(&conn, "status", if status == ScanStatus::Complete { "complete" } else { "partial" })?;
    let stats = stats(&conn)?;
    let hardlinks = meta_get(&conn, "hardlinks")?.as_deref() == Some("detected");
    Ok(ScanSummary {
        status,
        root,
        volume_serial: vol.serial,
        filesystem: vol.filesystem,
        cluster_size: vol.cluster_size,
        hardlinks_deduplicated: hardlinks,
        stats,
    })
}

/// Root row (id 0) from the unit totals.
fn write_root(conn: &mut Connection, root: &str) -> rusqlite::Result<()> {
    let mut t = DirTotals::default();
    let mut stmt = conn.prepare(
        "SELECT block, size, alloc, file_count, dir_count, max_mtime, max_atime FROM units WHERE status = 'done'",
    )?;
    let units: Vec<(i64, DirTotals)> = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                DirTotals {
                    size: r.get::<_, i64>(1)? as u64,
                    alloc: r.get::<_, i64>(2)? as u64,
                    files: r.get::<_, i64>(3)? as u64,
                    dirs: r.get::<_, i64>(4)? as u64,
                    max_mtime: r.get(5)?,
                    max_atime: r.get(6)?,
                    last_desc_id: 0,
                    cats: [0; 10],
                },
            ))
        })?
        .collect::<Result<_, _>>()?;
    drop(stmt);
    for (block, u) in units {
        if block == 0 {
            t.size += u.size;
            t.alloc += u.alloc;
            t.files += u.files;
            t.max_mtime = t.max_mtime.max(u.max_mtime);
            t.max_atime = t.max_atime.max(u.max_atime);
        } else {
            add_dir(&mut t, &u);
        }
    }
    t.last_desc_id = conn.query_row("SELECT coalesce(max(id), 0) FROM entries", [], |r| r.get(0))?;
    let md = std::fs::metadata(to_extended(root)).ok();
    let secs = |t: Option<std::time::SystemTime>| {
        t.and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs() as i64).unwrap_or(0)
    };
    let meta = FindEntry {
        name: root.to_string(),
        attrs: alloc::FILE_ATTRIBUTE_DIRECTORY,
        size: 0,
        ctime: secs(md.as_ref().and_then(|m| m.created().ok())),
        atime: secs(md.as_ref().and_then(|m| m.accessed().ok())),
        mtime: secs(md.as_ref().and_then(|m| m.modified().ok())),
        reparse_tag: 0,
    };
    let mut row = dir_row(0, 0, 0, &meta, root, &t, 0);
    row.parent_id = None;
    store::insert_rows(conn, &[row])
}

fn stats(conn: &Connection) -> rusqlite::Result<ScanStats> {
    conn.query_row(
        "SELECT
           (SELECT count(*) FROM entries WHERE kind = 0 AND hardlink_of IS NULL),
           (SELECT count(*) FROM entries WHERE kind = 1 AND id != 0),
           (SELECT count(*) FROM entries WHERE kind = 2),
           (SELECT coalesce(sum(size), 0) FROM entries WHERE kind = 0 AND hardlink_of IS NULL),
           (SELECT coalesce(sum(alloc), 0) FROM entries WHERE kind = 0 AND hardlink_of IS NULL),
           (SELECT count(*) FROM errors),
           (SELECT count(*) FROM entries WHERE flags & 1),
           (SELECT count(*) FROM entries WHERE flags & 4)",
        [],
        |r| {
            Ok(ScanStats {
                files: r.get::<_, i64>(0)? as u64,
                dirs: r.get::<_, i64>(1)? as u64,
                links: r.get::<_, i64>(2)? as u64,
                size: r.get::<_, i64>(3)? as u64,
                alloc: r.get::<_, i64>(4)? as u64,
                errors: r.get::<_, i64>(5)? as u64,
                access_denied: r.get::<_, i64>(6)? as u64,
                link_count_unknown: r.get::<_, i64>(7)? as u64,
            })
        },
    )
}

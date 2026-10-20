//! Build a snapshot database from an MFT source, in two passes.
//!
//! Snapshot ids are depth-first pre-order so that a folder's subtree is the id
//! range `[id, last_desc_id]`. Pre-order needs the whole tree before the first
//! row is written, but holding every name and size for millions of records
//! would not fit the memory bound. So:
//!
//! 1. **Index.** Stream the entries once and keep only a few bytes per record
//!    (sequence number, kind, one `(parent, record, name#)` triple per name) plus the names
//!    of folders. Then sort, validate parents and walk the tree to assign ids.
//! 2. **Write.** Stream the entries again; every name now has its id, parent id and
//!    folder path, so rows can be written straight to the snapshot and folder totals
//!    accumulated in a vector indexed by folder.
//!
//! Hard links: a file with several names gets one row per name; the name with
//! the lowest id is the one that counts, the rest point at it with `hardlink_of`.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

use super::model::{MftEntry, MftError, MftInfo, MftSource, ROOT_FRN};
use crate::alloc;
use crate::category::{self, SystemPaths};
use crate::paths;
use crate::reparse::{self, Action};
use crate::scan::{add_dir, add_file, meta_set, stats, ScanStatus, ScanSummary};
use crate::store::{self, DirTotals, Row, BATCH_ROWS, FLAG_CLOUD_PLACEHOLDER, KIND_DIR, KIND_FILE, KIND_LINK};

pub const META_NAME: &str = "<NTFS metadata>";
const NONE: u32 = u32::MAX;
const PARENT_META: u32 = u32::MAX - 1;
const MAX_NAMES: usize = 250;
const CHECK_EVERY: u64 = 8192;

#[derive(Debug, Clone)]
pub struct ImportOptions {
    /// Volume root such as `C:\`.
    pub root: String,
    pub db_path: PathBuf,
    pub windows_dir: Option<String>,
    /// Give up (so the caller falls back to the walk) above this share of unreadable records.
    pub max_bad_fraction: f64,
}

#[derive(Debug, thiserror::Error)]
pub enum ImportError {
    #[error(transparent)]
    Mft(#[from] MftError),
    #[error("snapshot database error: {0}")]
    Db(String),
    #[error("cancelled")]
    Cancelled,
}

impl From<rusqlite::Error> for ImportError {
    fn from(e: rusqlite::Error) -> Self {
        ImportError::Db(e.to_string())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    Indexing,
    Writing,
}

/// Reparse entries the walk records as links and never enters.
pub fn is_link(e: &MftEntry) -> bool {
    e.reparse_tag != 0 && reparse::action(e.reparse_tag, e.is_dir) == Action::Link
}

/// Kind codes in the index: 1 file, 2 folder, 3 link.
fn kind_of(e: &MftEntry) -> u8 {
    if is_link(e) {
        3
    } else if e.is_dir {
        2
    } else {
        1
    }
}

/// Allocated size exactly as the walk derives it from what it can know.
pub fn allocated_for(e: &MftEntry, cluster: u64) -> u64 {
    // Compressed and sparse streams report their real allocation. Dedup and WOF files keep
    // their data elsewhere; the walk asks Windows for it, the MFT cannot say, so they count at logical size.
    let queried =
        (alloc::needs_query(e.attrs, e.reparse_tag) && !e.resident && !reparse::needs_compressed_size(e.reparse_tag))
            .then_some(e.allocated);
    alloc::allocated(e.size, e.attrs, cluster, queried)
}

/// Where a name sits in the new tree.
pub struct Place<'a> {
    pub id: i64,
    pub parent: i64,
    pub depth: i64,
    pub dir_path: &'a str,
}

/// Build the file or link row for one name of `e`.
pub fn name_row(e: &MftEntry, name: &str, at: &Place<'_>, cluster: u64, system: &SystemPaths) -> Row {
    let Place { id, parent, depth, dir_path } = *at;
    if is_link(e) {
        return Row {
            id,
            parent_id: Some(parent),
            name: name.to_string(),
            kind: KIND_LINK,
            depth,
            ctime: e.ctime,
            mtime: e.mtime,
            atime: e.atime,
            attrs: e.attrs,
            reparse_tag: e.reparse_tag,
            frn: Some(e.frn),
            ..Row::default()
        };
    }
    let ext = category::extension(name);
    let cat = category::categorize(system, dir_path, name, &ext);
    let flags = if alloc::is_cloud_placeholder(e.attrs) { FLAG_CLOUD_PLACEHOLDER } else { 0 };
    Row {
        id,
        parent_id: Some(parent),
        name: name.to_string(),
        kind: KIND_FILE,
        depth,
        size: e.size,
        alloc: allocated_for(e, cluster),
        ctime: e.ctime,
        mtime: e.mtime,
        atime: e.atime,
        attrs: e.attrs,
        reparse_tag: e.reparse_tag,
        ext: Some(ext),
        category: Some(cat as u8),
        flags,
        frn: Some(e.frn),
        ..Row::default()
    }
}

/// `(parent record, record, name number)` for one name of one record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Ent {
    parent: u32,
    frn: u32,
    idx: u8,
    parent_seq: u16,
}

struct DirNode {
    frn: u32,
    eid: u32,
    parent_ord: u32,
    depth: i64,
    name: String,
    path: String,
    last_desc: u32,
    ctime: i64,
    mtime: i64,
    atime: i64,
    attrs: u32,
    totals: DirTotals,
}

#[derive(Default)]
struct Index {
    seq: Vec<u16>,
    kind: Vec<u8>,
    ents: Vec<Ent>,
    dir_names: HashMap<u32, String>,
    max_frn: usize,
    skipped: u64,
}

impl Index {
    fn add(&mut self, e: &MftEntry) {
        if e.frn >= u64::from(PARENT_META) - 4 || e.names.is_empty() {
            self.skipped += 1;
            return;
        }
        let f = e.frn as usize;
        if f >= self.kind.len() {
            let n = (f + 1).max(self.kind.len() + self.kind.len() / 4);
            self.kind.resize(n, 0);
            self.seq.resize(n, 0);
        }
        self.max_frn = self.max_frn.max(f);
        let kind = kind_of(e);
        self.kind[f] = kind;
        self.seq[f] = e.seq;
        if e.frn == ROOT_FRN {
            return;
        }
        if kind == 2 {
            self.dir_names.insert(f as u32, e.names[0].name.clone());
        }
        let take = if e.is_metafile() { 1 } else { e.names.len().min(MAX_NAMES) };
        for (i, n) in e.names.iter().enumerate().take(take) {
            if n.parent >= u64::from(PARENT_META) - 4 {
                continue;
            }
            let parent = if e.is_metafile() { PARENT_META } else { n.parent as u32 };
            self.ents.push(Ent { parent, frn: f as u32, idx: i as u8, parent_seq: n.parent_seq });
        }
    }
}

fn find_ent(ents: &[Ent], start: &[u32], parent: u32, frn: u32, idx: u8) -> Option<usize> {
    let (lo, hi) = (start[parent as usize] as usize, start[parent as usize + 1] as usize);
    let part = &ents[lo..hi];
    let at = part.partition_point(|e| (e.frn, e.idx) < (frn, idx));
    (part.get(at).is_some_and(|e| e.frn == frn && e.idx == idx)).then_some(lo + at)
}

/// Import every entry from `src` into a new snapshot database.
pub fn import(
    src: &mut dyn MftSource,
    opts: &ImportOptions,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(Stage, u64),
) -> Result<ScanSummary, ImportError> {
    let info: MftInfo = src.info().clone();
    let cluster = info.bytes_per_cluster;
    let root = opts.root.clone();

    // ---- Pass 1: index ----------------------------------------------------------------
    let mut ix = Index::default();
    let reserve = info.record_count.min(50_000_000) as usize;
    ix.kind.resize(reserve.max(1024), 0);
    ix.seq.resize(reserve.max(1024), 0);
    ix.ents.reserve(reserve / 2);
    let mut seen = 0u64;
    let tally = src.for_each(&mut |e| {
        ix.add(&e);
        seen += 1;
        if seen.is_multiple_of(CHECK_EVERY) {
            progress(Stage::Indexing, seen);
            if cancel.load(Ordering::Relaxed) {
                return false;
            }
        }
        true
    })?;
    if cancel.load(Ordering::Relaxed) {
        return Err(ImportError::Cancelled);
    }
    let bad = tally.bad_records + ix.skipped;
    let total = tally.records.max(1) + tally.bad_records;
    if bad as f64 / total as f64 > opts.max_bad_fraction {
        return Err(MftError::TooManyBadRecords { bad, total }.into());
    }
    if ix.kind.get(ROOT_FRN as usize).copied() != Some(2) {
        return Err(MftError::Other("the root folder record was not found".into()).into());
    }

    let meta_idx = ix.max_frn + 1;
    let slots = meta_idx + 1;
    ix.kind.resize(slots + 1, 0);
    ix.seq.resize(slots + 1, 0);
    ix.kind[meta_idx] = 2;
    ix.dir_names.insert(meta_idx as u32, META_NAME.to_string());
    let root_seq = ix.seq[ROOT_FRN as usize];
    ix.ents.push(Ent { parent: ROOT_FRN as u32, frn: meta_idx as u32, idx: 0, parent_seq: root_seq });
    let meta_seq = ix.seq[meta_idx];
    for e in &mut ix.ents {
        if e.parent == PARENT_META {
            e.parent = meta_idx as u32;
            e.parent_seq = meta_seq;
        }
    }
    let Index { kind, seq, mut ents, dir_names, .. } = ix;
    // A name counts only when its parent is a live folder with the same sequence number.
    ents.retain(|e| {
        let p = e.parent as usize;
        p < slots && kind[p] == 2 && seq[p] == e.parent_seq && kind[e.frn as usize] != 0
    });
    ents.sort_unstable();

    let mut start = vec![0u32; slots + 2];
    for e in &ents {
        start[e.parent as usize + 1] += 1;
    }
    for i in 1..start.len() {
        start[i] += start[i - 1];
    }

    // Depth-first pre-order ids.
    let mut eid = vec![NONE; ents.len()];
    let mut ent_last = vec![0u32; ents.len()];
    let mut ord_of = vec![NONE; slots + 1];
    let mut dirs: Vec<DirNode> = vec![DirNode {
        frn: ROOT_FRN as u32,
        eid: 0,
        parent_ord: NONE,
        depth: 0,
        name: root.clone(),
        path: root.clone(),
        last_desc: 0,
        ctime: 0,
        mtime: 0,
        atime: 0,
        attrs: alloc::FILE_ATTRIBUTE_DIRECTORY,
        totals: DirTotals::default(),
    }];
    ord_of[ROOT_FRN as usize] = 0;
    let mut counter: u32 = 1;
    // (dir ordinal, entry index in its parent that created it, next child, end)
    let mut stack: Vec<(u32, u32, u32, u32)> = vec![(0, NONE, start[ROOT_FRN as usize], start[ROOT_FRN as usize + 1])];
    while let Some(top) = stack.last_mut() {
        if top.2 >= top.3 {
            let (ord, via, _, _) = *top;
            stack.pop();
            dirs[ord as usize].last_desc = counter - 1;
            if via != NONE {
                ent_last[via as usize] = counter - 1;
            }
            continue;
        }
        let ent_idx = top.2 as usize;
        top.2 += 1;
        let parent_ord = top.0;
        let e = ents[ent_idx];
        let f = e.frn as usize;
        match kind[f] {
            2 => {
                if ord_of[f] != NONE {
                    continue; // a folder with two names is damaged; keep the first
                }
                let name = dir_names.get(&e.frn).cloned().unwrap_or_else(|| format!("?{}", e.frn));
                let p = &dirs[parent_ord as usize];
                let path = paths::join(&p.path, &name);
                let ord = dirs.len() as u32;
                ord_of[f] = ord;
                eid[ent_idx] = counter;
                dirs.push(DirNode {
                    frn: e.frn,
                    eid: counter,
                    parent_ord,
                    depth: p.depth + 1,
                    name,
                    path,
                    last_desc: counter,
                    ctime: 0,
                    mtime: 0,
                    atime: 0,
                    attrs: if f == meta_idx { 0x16 } else { alloc::FILE_ATTRIBUTE_DIRECTORY },
                    totals: DirTotals::default(),
                });
                counter += 1;
                stack.push((ord, ent_idx as u32, start[f], start[f + 1]));
            }
            _ => {
                eid[ent_idx] = counter;
                counter += 1;
            }
        }
    }
    drop(dir_names);
    drop(seq);

    // ---- Pass 2: write ----------------------------------------------------------------
    let mut conn = store::open(&opts.db_path)?;
    meta_set(&conn, "root", &root)?;
    meta_set(&conn, "volume_serial", &info.serial.to_string())?;
    meta_set(&conn, "filesystem", "NTFS")?;
    meta_set(&conn, "cluster_size", &cluster.to_string())?;
    meta_set(&conn, "hardlinks", "detected")?;
    meta_set(&conn, "scan_method", "mft")?;
    meta_set(&conn, "status", "running")?;

    let system = opts.windows_dir.as_deref().map(SystemPaths::new).unwrap_or_else(SystemPaths::from_env);
    let mut rows: Vec<Row> = Vec::with_capacity(BATCH_ROWS);
    let mut db_err: Option<String> = None;
    let mut written = 0u64;
    src.for_each(&mut |e| {
        written += 1;
        if written.is_multiple_of(CHECK_EVERY) {
            progress(Stage::Writing, written);
            if cancel.load(Ordering::Relaxed) {
                return false;
            }
        }
        let f = e.frn as usize;
        if e.frn == ROOT_FRN {
            let d = &mut dirs[0];
            (d.ctime, d.mtime, d.atime) = (e.ctime, e.mtime, e.atime);
            d.attrs = e.attrs | alloc::FILE_ATTRIBUTE_DIRECTORY;
            return true;
        }
        if f >= slots {
            return true;
        }
        // Resolve each name to its entry.
        let take = if e.is_metafile() { 1 } else { e.names.len().min(MAX_NAMES) };
        let mut resolved: Vec<(usize, u32, usize)> = Vec::new(); // (name index, entry index, parent ordinal)
        for (i, n) in e.names.iter().enumerate().take(take) {
            let parent = if e.is_metafile() {
                meta_idx as u32
            } else if n.parent < slots as u64 {
                n.parent as u32
            } else {
                continue;
            };
            if let Some(at) = find_ent(&ents, &start, parent, e.frn as u32, i as u8) {
                let pord = ord_of[parent as usize];
                if eid[at] != NONE && pord != NONE {
                    resolved.push((i, at as u32, pord as usize));
                }
            }
        }
        if resolved.is_empty() {
            return true;
        }
        if e.is_dir && !is_link(&e) {
            if let Some(&(_, _, _)) = resolved.first() {
                let ord = ord_of[f];
                if ord != NONE {
                    let d = &mut dirs[ord as usize];
                    (d.ctime, d.mtime, d.atime, d.attrs) =
                        (e.ctime, e.mtime, e.atime, e.attrs | alloc::FILE_ATTRIBUTE_DIRECTORY);
                }
            }
            return true;
        }
        // Files and links: one row per name; the lowest id is the one that counts.
        let canonical = resolved.iter().map(|r| eid[r.1 as usize]).min().unwrap_or(NONE);
        let links = resolved.len();
        for &(i, at, pord) in &resolved {
            let id = eid[at as usize];
            let (parent_eid, depth, dir_path) = {
                let p = &dirs[pord];
                (i64::from(p.eid), p.depth + 1, p.path.as_str())
            };
            let at = Place { id: i64::from(id), parent: parent_eid, depth, dir_path };
            let mut row = name_row(&e, &e.names[i].name, &at, cluster, &system);
            let counts = id == canonical;
            if links > 1 {
                row.nlink = Some(links as u32);
                row.file_id = Some(u128::from(e.frn) | (u128::from(e.seq) << 48));
                if !counts {
                    row.hardlink_of = Some(i64::from(canonical));
                }
            }
            if row.kind == KIND_FILE {
                add_file(&mut dirs[pord].totals, &row, counts);
            }
            rows.push(row);
        }
        if rows.len() >= BATCH_ROWS {
            if let Err(err) = store::insert_rows(&mut conn, &rows) {
                db_err = Some(err.to_string());
                return false;
            }
            rows.clear();
        }
        true
    })?;
    if let Some(err) = db_err {
        return Err(ImportError::Db(err));
    }
    if cancel.load(Ordering::Relaxed) {
        return Err(ImportError::Cancelled);
    }
    if !rows.is_empty() {
        store::insert_rows(&mut conn, &rows)?;
        rows.clear();
    }

    // Totals bottom-up (children always have a larger ordinal than their parent), then the folder rows.
    for ord in (1..dirs.len()).rev() {
        let (child, parent) = (dirs[ord].totals, dirs[ord].parent_ord as usize);
        dirs[ord].totals.last_desc_id = i64::from(dirs[ord].last_desc);
        add_dir(&mut dirs[parent].totals, &child);
    }
    dirs[0].totals.last_desc_id = i64::from(dirs[0].last_desc);
    for chunk in dirs.chunks(BATCH_ROWS) {
        let batch: Vec<Row> = chunk
            .iter()
            .map(|d| Row {
                id: i64::from(d.eid),
                parent_id: (d.parent_ord != NONE).then(|| i64::from(dirs[d.parent_ord as usize].eid)),
                name: d.name.clone(),
                kind: KIND_DIR,
                depth: d.depth,
                size: d.totals.size,
                alloc: d.totals.alloc,
                ctime: d.ctime,
                mtime: d.mtime,
                atime: d.atime,
                attrs: d.attrs,
                path: Some(d.path.clone()),
                dir: Some(d.totals),
                frn: (d.frn as usize != meta_idx).then_some(u64::from(d.frn)),
                ..Row::default()
            })
            .collect();
        store::insert_rows(&mut conn, &batch)?;
    }

    conn.execute_batch("CREATE INDEX IF NOT EXISTS entries_frn ON entries(frn) WHERE frn IS NOT NULL")?;
    store::finalize(&conn)?;
    meta_set(&conn, "status", "complete")?;
    let st = stats(&conn)?;
    Ok(ScanSummary {
        status: ScanStatus::Complete,
        root,
        volume_serial: info.serial,
        filesystem: "NTFS".into(),
        cluster_size: cluster,
        hardlinks_deduplicated: true,
        stats: st,
    })
}

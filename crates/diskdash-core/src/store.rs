//! Snapshot database: the batched writer thread and finalisation.

use std::path::Path;
use std::thread::JoinHandle;

use crossbeam_channel::{bounded, Receiver, Sender};
use rusqlite::{params, Connection};

pub const SCHEMA: &str = include_str!("schema.sql");
pub const BATCH_ROWS: usize = 10_000;

pub const FLAG_ACCESS_DENIED: i64 = 1;
pub const FLAG_CLOUD_PLACEHOLDER: i64 = 2;
pub const FLAG_LINK_COUNT_UNKNOWN: i64 = 4;

pub const KIND_FILE: i64 = 0;
pub const KIND_DIR: i64 = 1;
pub const KIND_LINK: i64 = 2;

#[derive(Debug, Clone, Default)]
pub struct Row {
    pub id: i64,
    pub parent_id: Option<i64>,
    pub name: String,
    pub kind: i64,
    pub depth: i64,
    pub size: u64,
    pub alloc: u64,
    pub ctime: i64,
    pub mtime: i64,
    pub atime: i64,
    pub attrs: u32,
    pub reparse_tag: u32,
    pub file_id: Option<u128>,
    pub nlink: Option<u32>,
    pub hardlink_of: Option<i64>,
    pub ext: Option<String>,
    pub category: Option<u8>,
    pub flags: i64,
    pub link_target: Option<String>,
    pub path: Option<String>,
    pub dir: Option<DirTotals>,
    /// MFT record number (MFT scans only).
    pub frn: Option<u64>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DirTotals {
    pub size: u64,
    pub alloc: u64,
    pub files: u64,
    pub dirs: u64,
    pub max_mtime: i64,
    pub max_atime: i64,
    pub last_desc_id: i64,
    /// Allocated bytes per file category (index = `Category as u8`).
    pub cats: [u64; 10],
}

impl DirTotals {
    /// Category holding the most bytes below this directory.
    pub fn top_category(&self) -> Option<u8> {
        let (i, max) = self.cats.iter().enumerate().max_by_key(|(_, v)| **v)?;
        (*max > 0).then_some(i as u8)
    }

    pub fn add(&mut self, o: &DirTotals) {
        for (a, b) in self.cats.iter_mut().zip(o.cats.iter()) {
            *a += b;
        }
        self.size += o.size;
        self.alloc += o.alloc;
        self.files += o.files;
        self.dirs += o.dirs;
        self.max_mtime = self.max_mtime.max(o.max_mtime);
        self.max_atime = self.max_atime.max(o.max_atime);
        self.last_desc_id = self.last_desc_id.max(o.last_desc_id);
    }
}

#[derive(Debug, Clone)]
pub struct ErrRow {
    pub block: i64,
    pub path: String,
    pub op: &'static str,
    pub code: u32,
    pub message: String,
}

pub enum Msg {
    Rows(Vec<Row>),
    Errors(Vec<ErrRow>),
    UnitDone { block: i64, totals: DirTotals },
}

fn nz(v: i64) -> Option<i64> {
    (v != 0).then_some(v)
}

/// Open (creating) a snapshot database tuned for bulk loading.
pub fn open(path: &Path) -> rusqlite::Result<Connection> {
    let conn = Connection::open(path)?;
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "OFF")?;
    conn.pragma_update(None, "temp_store", "MEMORY")?;
    conn.pragma_update(None, "cache_size", -64_000)?;
    conn.execute_batch(SCHEMA)?;
    // Snapshots created before MFT scans existed lack the record-number column.
    let has_frn: bool = conn.prepare("SELECT 1 FROM pragma_table_info('entries') WHERE name = 'frn'")?.exists([])?;
    if !has_frn {
        conn.execute_batch("ALTER TABLE entries ADD COLUMN frn INTEGER")?;
    }
    Ok(conn)
}

pub fn insert_rows(conn: &mut Connection, rows: &[Row]) -> rusqlite::Result<()> {
    let tx = conn.transaction()?;
    {
        let mut stmt = tx.prepare_cached(
            "INSERT OR REPLACE INTO entries (id, parent_id, name, name_lower, kind, depth, size, alloc, ctime, mtime, atime,
              attrs, reparse_tag, file_id, nlink, hardlink_of, ext, category, flags, link_target, path,
              file_count, dir_count, max_mtime, max_atime, last_desc_id, top_category, frn)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21,
                     ?22, ?23, ?24, ?25, ?26, ?27, ?28)",
        )?;
        for r in rows {
            let d = r.dir;
            stmt.execute(params![
                r.id,
                r.parent_id,
                r.name,
                r.name.to_lowercase(),
                r.kind,
                r.depth,
                r.size as i64,
                r.alloc as i64,
                nz(r.ctime),
                nz(r.mtime),
                nz(r.atime),
                i64::from(r.attrs),
                i64::from(r.reparse_tag),
                r.file_id.map(|f| format!("{f:032x}")),
                r.nlink.map(i64::from),
                r.hardlink_of,
                r.ext,
                r.category.map(i64::from),
                r.flags,
                r.link_target,
                r.path,
                d.map(|d| d.files as i64),
                d.map(|d| d.dirs as i64),
                d.map(|d| d.max_mtime),
                d.map(|d| d.max_atime),
                d.map(|d| d.last_desc_id),
                d.and_then(|d| d.top_category()).map(i64::from),
                r.frn.map(|f| f as i64),
            ])?;
        }
    }
    tx.commit()
}

fn insert_errors(conn: &mut Connection, errs: &[ErrRow]) -> rusqlite::Result<()> {
    let tx = conn.transaction()?;
    {
        let mut stmt =
            tx.prepare_cached("INSERT INTO errors (block, path, op, code, message) VALUES (?1, ?2, ?3, ?4, ?5)")?;
        for e in errs {
            stmt.execute(params![e.block, e.path, e.op, i64::from(e.code), e.message])?;
        }
    }
    tx.commit()
}

pub fn mark_unit_done(conn: &Connection, block: i64, t: &DirTotals) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE units SET status = 'done', size = ?2, alloc = ?3, file_count = ?4, dir_count = ?5,
           max_mtime = ?6, max_atime = ?7, last_id = ?8 WHERE block = ?1",
        params![
            block,
            t.size as i64,
            t.alloc as i64,
            t.files as i64,
            t.dirs as i64,
            t.max_mtime,
            t.max_atime,
            t.last_desc_id
        ],
    )?;
    Ok(())
}

/// Single writer thread fed through a bounded channel (backpressure: walkers
/// block when the disk cannot keep up, memory stays bounded).
pub struct Writer {
    tx: Option<Sender<Msg>>,
    handle: Option<JoinHandle<rusqlite::Result<Connection>>>,
}

impl Writer {
    pub fn start(conn: Connection, capacity: usize) -> Self {
        let (tx, rx): (Sender<Msg>, Receiver<Msg>) = bounded(capacity);
        let handle = std::thread::Builder::new()
            .name("snapshot-writer".into())
            .spawn(move || {
                let mut conn = conn;
                for msg in rx {
                    match msg {
                        Msg::Rows(rows) => insert_rows(&mut conn, &rows)?,
                        Msg::Errors(errs) => insert_errors(&mut conn, &errs)?,
                        Msg::UnitDone { block, totals } => mark_unit_done(&conn, block, &totals)?,
                    }
                }
                Ok(conn)
            })
            .ok();
        Self { tx: Some(tx), handle }
    }

    pub fn sender(&self) -> Sender<Msg> {
        self.tx.clone().expect("writer running")
    }

    /// Close the channel and wait for everything to be written.
    pub fn finish(mut self) -> Result<Connection, String> {
        drop(self.tx.take());
        match self.handle.take() {
            Some(h) => h.join().map_err(|_| "writer thread panicked".to_string())?.map_err(|e| e.to_string()),
            None => Err("writer thread did not start".into()),
        }
    }
}

/// Create query indexes and precompute breakdowns (after bulk load: faster).
pub fn finalize(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS entries_children_alloc ON entries(parent_id, alloc DESC);
         CREATE INDEX IF NOT EXISTS entries_children_size ON entries(parent_id, size DESC);
         CREATE INDEX IF NOT EXISTS entries_kind_alloc ON entries(kind, alloc DESC);
         CREATE INDEX IF NOT EXISTS entries_kind_size ON entries(kind, size DESC);
         CREATE INDEX IF NOT EXISTS entries_name_lower ON entries(name_lower);
         CREATE INDEX IF NOT EXISTS entries_dir_path ON entries(path) WHERE path IS NOT NULL;
         DELETE FROM category_totals;
         INSERT INTO category_totals (category, file_count, size, alloc)
           SELECT coalesce(category, 9), count(*), sum(size), sum(alloc) FROM entries
           WHERE kind = 0 AND hardlink_of IS NULL GROUP BY coalesce(category, 9);
         DELETE FROM ext_totals;
         INSERT INTO ext_totals (ext, category, file_count, size, alloc)
           SELECT coalesce(ext, ''), coalesce(max(category), 9), count(*), sum(size), sum(alloc) FROM entries
           WHERE kind = 0 AND hardlink_of IS NULL GROUP BY coalesce(ext, '');
         UPDATE entries SET top_category = (SELECT category FROM category_totals ORDER BY alloc DESC LIMIT 1)
           WHERE id = 0;
         PRAGMA optimize;",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writer_persists_rows_errors_and_units() {
        let tmp = tempfile::tempdir().unwrap();
        let conn = open(&tmp.path().join("s.db")).unwrap();
        conn.execute("INSERT INTO units (block, path, status) VALUES (1, 'x', 'pending')", []).unwrap();
        let w = Writer::start(conn, 2);
        let tx = w.sender();
        let rows: Vec<Row> =
            (0..25_000).map(|i| Row { id: i, name: format!("f{i}"), size: 1, alloc: 4096, ..Row::default() }).collect();
        for chunk in rows.chunks(BATCH_ROWS) {
            tx.send(Msg::Rows(chunk.to_vec())).unwrap();
        }
        tx.send(Msg::Errors(vec![ErrRow {
            block: 1,
            path: "p".into(),
            op: "list",
            code: 5,
            message: "denied".into(),
        }]))
        .unwrap();
        tx.send(Msg::UnitDone { block: 1, totals: DirTotals { size: 7, ..Default::default() } }).unwrap();
        drop(tx);
        let conn = w.finish().unwrap();
        let n: i64 = conn.query_row("SELECT count(*) FROM entries", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 25_000);
        let status: String = conn.query_row("SELECT status FROM units WHERE block = 1", [], |r| r.get(0)).unwrap();
        assert_eq!(status, "done");
        let e: i64 = conn.query_row("SELECT count(*) FROM errors", [], |r| r.get(0)).unwrap();
        assert_eq!(e, 1);
    }

    #[test]
    fn bounded_channel_applies_backpressure() {
        let (tx, rx) = bounded::<Msg>(1);
        tx.send(Msg::Rows(vec![])).unwrap();
        assert!(tx.try_send(Msg::Rows(vec![])).is_err(), "producer must block when the writer lags");
        drop(rx);
    }
}

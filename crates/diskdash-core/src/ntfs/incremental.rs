//! Quick rescan: a new snapshot from the previous one plus what the USN
//! journal says changed.
//!
//! The previous snapshot's rows are replayed as MFT entries, the changed
//! records replace or remove their entries, and the usual importer builds the new
//! snapshot. The new snapshot therefore has the same ids, totals, hard-link
//! choices and indexes as a full scan would give; only the changed records were
//! read from the volume.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::atomic::AtomicBool;

use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};

use super::import::{import, ImportError, ImportOptions, Stage};
use super::model::{EntryName, MftEntry, MftError, MftInfo, MftSource, ScanTally, ROOT_FRN};
use crate::alloc::FILE_ATTRIBUTE_DIRECTORY;
use crate::scan::ScanSummary;
use crate::store::KIND_DIR;

/// The current state of the records the journal named.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Changes {
    /// Records that exist now (new or modified), complete.
    pub entries: Vec<MftEntry>,
    /// Records that are gone (deleted or no longer in use).
    pub gone: Vec<u64>,
}

/// Replays a snapshot database as an MFT source, with `changes` applied on top.
pub struct DbSource {
    conn: Connection,
    info: MftInfo,
    root_attrs: (u32, i64, i64, i64),
    overlay: BTreeMap<u64, Option<MftEntry>>,
}

impl DbSource {
    pub fn open(db: &Path, changes: Changes) -> Result<Self, String> {
        let conn = Connection::open_with_flags(db, OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(|e| e.to_string())?;
        let meta = |k: &str| -> Option<String> {
            conn.query_row("SELECT value FROM meta WHERE key = ?1", [k], |r| r.get(0)).ok()
        };
        let cluster: u64 = meta("cluster_size").and_then(|v| v.parse().ok()).ok_or("snapshot has no cluster size")?;
        let serial: u32 = meta("volume_serial").and_then(|v| v.parse().ok()).unwrap_or(0);
        let max_frn: i64 =
            conn.query_row("SELECT coalesce(max(frn), 0) FROM entries", [], |r| r.get(0)).map_err(|e| e.to_string())?;
        if conn
            .query_row("SELECT count(*) FROM entries WHERE frn IS NOT NULL", [], |r| r.get::<_, i64>(0))
            .map_err(|e| e.to_string())?
            == 0
        {
            return Err("the previous snapshot was not made by an MFT scan".into());
        }
        let root_attrs = conn
            .query_row("SELECT attrs, ctime, mtime, atime FROM entries WHERE id = 0", [], |r| {
                Ok((
                    r.get::<_, i64>(0)? as u32,
                    r.get::<_, Option<i64>>(1)?.unwrap_or(0),
                    r.get::<_, Option<i64>>(2)?.unwrap_or(0),
                    r.get::<_, Option<i64>>(3)?.unwrap_or(0),
                ))
            })
            .map_err(|e| e.to_string())?;
        let mut overlay: BTreeMap<u64, Option<MftEntry>> = BTreeMap::new();
        for g in changes.gone {
            overlay.insert(g, None);
        }
        let mut max_changed = 0;
        for mut e in changes.entries {
            // Sequence numbers are not stored in snapshots; compare nothing across the two sources.
            e.seq = 0;
            for n in &mut e.names {
                n.parent_seq = 0;
            }
            max_changed = max_changed.max(e.frn);
            overlay.insert(e.frn, Some(e));
        }
        Ok(Self {
            conn,
            info: MftInfo {
                bytes_per_cluster: cluster,
                record_size: 1024,
                record_count: (max_frn as u64).max(max_changed) + 1,
                serial,
            },
            root_attrs,
            overlay,
        })
    }
}

impl MftSource for DbSource {
    fn info(&self) -> &MftInfo {
        &self.info
    }

    fn for_each(&mut self, f: &mut dyn FnMut(MftEntry) -> bool) -> Result<ScanTally, MftError> {
        let (attrs, ctime, mtime, atime) = self.root_attrs;
        let root = MftEntry {
            frn: ROOT_FRN,
            seq: 0,
            is_dir: true,
            attrs: attrs | FILE_ATTRIBUTE_DIRECTORY,
            ctime,
            mtime,
            atime,
            names: vec![EntryName { parent: ROOT_FRN, parent_seq: 0, name: ".".into() }],
            size: 0,
            allocated: 0,
            resident: true,
            compressed_or_sparse: false,
            reparse_tag: 0,
        };
        let mut count = 1u64;
        if !f(root) {
            return Ok(ScanTally { records: count, bad_records: 0 });
        }
        let mut stmt = self
            .conn
            .prepare(
                "SELECT e.frn, e.name, coalesce(p.frn, 5), e.kind, e.attrs, e.ctime, e.mtime, e.atime, e.size, e.alloc, e.reparse_tag
                 FROM entries e LEFT JOIN entries p ON p.id = e.parent_id
                 WHERE e.frn IS NOT NULL AND e.frn != 5 ORDER BY e.frn, e.id",
            )
            .map_err(|e| MftError::Read(e.to_string()))?;
        let mut rows = stmt.query([]).map_err(|e| MftError::Read(e.to_string()))?;
        let mut overlay = self.overlay.iter().peekable();
        let mut cur: Option<MftEntry> = None;
        let mut keep_going = true;

        // Emit overlay entries up to and including `upto` (all of them when None), then `old` unless replaced.
        macro_rules! emit {
            ($e:expr) => {{
                count += 1;
                if !f($e) {
                    keep_going = false;
                }
            }};
        }
        let mut flush_old =
            |old: Option<MftEntry>,
             overlay: &mut std::iter::Peekable<std::collections::btree_map::Iter<'_, u64, Option<MftEntry>>>,
             keep_going: &mut bool,
             count: &mut u64| {
                let Some(old) = old else { return };
                while let Some((&k, _)) = overlay.peek() {
                    if k >= old.frn {
                        break;
                    }
                    if let Some((_, Some(e))) = overlay.next() {
                        *count += 1;
                        if !f(e.clone()) {
                            *keep_going = false;
                        }
                    }
                }
                match overlay.peek() {
                    Some((&k, _)) if k == old.frn => {
                        if let Some((_, Some(e))) = overlay.next() {
                            *count += 1;
                            if !f(e.clone()) {
                                *keep_going = false;
                            }
                        }
                    }
                    _ => {
                        *count += 1;
                        if !f(old) {
                            *keep_going = false;
                        }
                    }
                }
            };

        while let Some(r) = rows.next().map_err(|e| MftError::Read(e.to_string()))? {
            if !keep_going {
                break;
            }
            let frn = r.get::<_, i64>(0).map_err(|e| MftError::Read(e.to_string()))? as u64;
            let name: String = r.get(1).map_err(|e| MftError::Read(e.to_string()))?;
            let parent = r.get::<_, i64>(2).map_err(|e| MftError::Read(e.to_string()))? as u64;
            match cur.as_mut() {
                Some(c) if c.frn == frn => {
                    c.names.push(EntryName { parent, parent_seq: 0, name });
                    continue;
                }
                _ => {}
            }
            let kind: i64 = r.get(3).map_err(|e| MftError::Read(e.to_string()))?;
            let attrs = r.get::<_, i64>(4).map_err(|e| MftError::Read(e.to_string()))? as u32;
            let get_t = |i: usize| r.get::<_, Option<i64>>(i).map(|v| v.unwrap_or(0));
            let is_dir = kind == KIND_DIR || attrs & FILE_ATTRIBUTE_DIRECTORY != 0;
            let entry = MftEntry {
                frn,
                seq: 0,
                is_dir,
                attrs,
                ctime: get_t(5).map_err(|e| MftError::Read(e.to_string()))?,
                mtime: get_t(6).map_err(|e| MftError::Read(e.to_string()))?,
                atime: get_t(7).map_err(|e| MftError::Read(e.to_string()))?,
                names: vec![EntryName { parent, parent_seq: 0, name }],
                size: if is_dir {
                    0
                } else {
                    r.get::<_, i64>(8).map_err(|e| MftError::Read(e.to_string()))?.max(0) as u64
                },
                allocated: if is_dir {
                    0
                } else {
                    r.get::<_, i64>(9).map_err(|e| MftError::Read(e.to_string()))?.max(0) as u64
                },
                resident: false,
                compressed_or_sparse: false,
                reparse_tag: r.get::<_, i64>(10).map_err(|e| MftError::Read(e.to_string()))? as u32,
            };
            let finished = cur.replace(entry);
            flush_old(finished, &mut overlay, &mut keep_going, &mut count);
        }
        flush_old(cur.take(), &mut overlay, &mut keep_going, &mut count);
        // New records beyond the last old one.
        for (_, e) in overlay {
            if !keep_going {
                break;
            }
            if let Some(e) = e {
                emit!(e.clone());
            }
        }
        Ok(ScanTally { records: count, bad_records: 0 })
    }
}

/// Build the new snapshot at `opts.db_path` from `previous` and `changes`.
pub fn quick_rescan(
    previous: &Path,
    changes: Changes,
    opts: &ImportOptions,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(Stage, u64),
) -> Result<ScanSummary, ImportError> {
    let mut src = DbSource::open(previous, changes).map_err(|e| ImportError::Mft(MftError::Other(e)))?;
    import(&mut src, opts, cancel, progress)
}

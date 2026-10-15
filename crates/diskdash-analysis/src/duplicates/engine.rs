//! Stages 2 and 3: partial then full BLAKE3 hashing, scheduled per physical disk.
//!
//! Files are opened through [`Source`] (read-only). A spinning disk gets one
//! reader at a time, in snapshot entry order; SSDs get a bounded pool.

use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Mutex;

use super::candidates::Candidate;
use super::{CHUNK, PARTIAL_BYTES};

const REPARSE: u32 = 0x400;
const CLOUD_ATTRS: u32 = 0x1000 | 0x40000 | 0x400000;

/// Volume serial number and file index: two names with the same id are one file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FileId {
    pub serial: u32,
    pub index: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenError {
    Denied,
    Sharing,
    NotFound,
    Other(String),
}

pub trait ReadSeek: Read + Seek + Send {}
impl<T: Read + Seek + Send> ReadSeek for T {}

/// A file opened for reading, with what the open handle reports about it.
pub struct OpenFile {
    pub size: u64,
    /// Last-modified time (unix seconds).
    pub mtime: i64,
    pub attrs: u32,
    pub id: Option<FileId>,
    pub read: Box<dyn ReadSeek>,
}

/// Read-only access to file contents.
pub trait Source: Send + Sync {
    /// File attributes without opening the file (so cloud placeholders are never opened).
    fn attributes(&self, path: &str) -> Result<u32, OpenError>;
    fn open(&self, path: &str) -> Result<OpenFile, OpenError>;
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CacheKey {
    pub serial: u32,
    pub index: u64,
    pub size: u64,
    pub mtime: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CachedHash {
    pub partial: Option<String>,
    pub full: Option<String>,
}

pub trait HashCache: Send + Sync {
    fn get(&self, key: &CacheKey) -> Option<CachedHash>;
    /// Merge: only the set fields are updated.
    fn put(&self, key: &CacheKey, value: &CachedHash);
}

#[derive(Default)]
pub struct MemoryCache(Mutex<HashMap<CacheKey, CachedHash>>);

impl MemoryCache {
    pub fn len(&self) -> usize {
        self.0.lock().map(|m| m.len()).unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl HashCache for MemoryCache {
    fn get(&self, key: &CacheKey) -> Option<CachedHash> {
        self.0.lock().ok()?.get(key).cloned()
    }

    fn put(&self, key: &CacheKey, value: &CachedHash) {
        if let Ok(mut m) = self.0.lock() {
            let e = m.entry(key.clone()).or_default();
            if value.partial.is_some() {
                e.partial = value.partial.clone();
            }
            if value.full.is_some() {
                e.full = value.full.clone();
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SkipReason {
    CloudPlaceholder,
    ReparsePoint,
    ChangedSinceScan,
    AccessDenied,
    SharingViolation,
    NotFound,
    /// Another name of a file already in the same size group (same volume serial and file id).
    HardLink,
    Error,
}

impl SkipReason {
    pub fn key(self) -> &'static str {
        match self {
            SkipReason::CloudPlaceholder => "cloud-placeholder",
            SkipReason::ReparsePoint => "reparse-point",
            SkipReason::ChangedSinceScan => "changed-since-scan",
            SkipReason::AccessDenied => "access-denied",
            SkipReason::SharingViolation => "sharing-violation",
            SkipReason::NotFound => "not-found",
            SkipReason::HardLink => "hard-link",
            SkipReason::Error => "error",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Skip {
    /// Index into the candidate list.
    pub candidate: usize,
    pub reason: SkipReason,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group {
    pub size: u64,
    pub hash: String,
    /// Indexes into the candidate list.
    pub members: Vec<usize>,
}

#[derive(Debug, Default)]
pub struct Outcome {
    pub groups: Vec<Group>,
    pub skipped: Vec<Skip>,
    pub bytes_read: u64,
    pub cache_hits: u64,
    pub files_hashed: u64,
}

#[derive(Debug)]
pub struct Cancelled;

pub struct EngineEnv<'a> {
    pub source: &'a dyn Source,
    pub cache: &'a dyn HashCache,
    pub cancelled: &'a (dyn Fn() -> bool + Sync),
    /// `progress(bytes_done, bytes_planned)`.
    pub progress: &'a (dyn Fn(u64, u64) + Sync),
    /// Parallel readers per SSD.
    pub ssd_readers: usize,
}

struct State {
    /// Partial hash per candidate.
    partial: Vec<Option<String>>,
    full: Vec<Option<String>>,
    ids: Vec<Option<FileId>>,
    skipped: Vec<Skip>,
}

fn hex(h: &blake3::Hash) -> String {
    h.to_hex().to_string()
}

fn map_open_error(e: &OpenError) -> SkipReason {
    match e {
        OpenError::Denied => SkipReason::AccessDenied,
        OpenError::Sharing => SkipReason::SharingViolation,
        OpenError::NotFound => SkipReason::NotFound,
        OpenError::Other(_) => SkipReason::Error,
    }
}

/// Open and verify a candidate: not a cloud placeholder or link, unchanged since the scan.
fn open_checked(env: &EngineEnv<'_>, c: &Candidate) -> Result<OpenFile, SkipReason> {
    let attrs = env.source.attributes(&c.path).map_err(|e| map_open_error(&e))?;
    if attrs & CLOUD_ATTRS != 0 {
        return Err(SkipReason::CloudPlaceholder);
    }
    if attrs & REPARSE != 0 {
        return Err(SkipReason::ReparsePoint);
    }
    let f = env.source.open(&c.path).map_err(|e| map_open_error(&e))?;
    // Re-check on the handle itself: a link or placeholder could have appeared since.
    if f.attrs & (CLOUD_ATTRS | REPARSE) != 0 {
        return Err(if f.attrs & CLOUD_ATTRS != 0 { SkipReason::CloudPlaceholder } else { SkipReason::ReparsePoint });
    }
    if f.size != c.size || f.mtime != c.mtime {
        return Err(SkipReason::ChangedSinceScan);
    }
    Ok(f)
}

/// First and last 64 KiB (the whole file when it is at most 128 KiB), plus the size.
fn partial_hash(f: &mut OpenFile) -> std::io::Result<String> {
    let mut h = blake3::Hasher::new();
    let size = f.size;
    f.read.seek(SeekFrom::Start(0))?;
    if size <= 2 * PARTIAL_BYTES {
        let mut buf = Vec::with_capacity(size as usize);
        (&mut f.read).take(size).read_to_end(&mut buf)?;
        h.update(&buf);
    } else {
        let mut buf = vec![0u8; PARTIAL_BYTES as usize];
        f.read.read_exact(&mut buf)?;
        h.update(&buf);
        f.read.seek(SeekFrom::Start(size - PARTIAL_BYTES))?;
        f.read.read_exact(&mut buf)?;
        h.update(&buf);
    }
    h.update(&size.to_le_bytes());
    Ok(hex(&h.finalize()))
}

fn full_hash(
    f: &mut OpenFile,
    cancelled: &(dyn Fn() -> bool + Sync),
    done: &AtomicU64,
    tick: &dyn Fn(),
) -> std::io::Result<Option<String>> {
    f.read.seek(SeekFrom::Start(0))?;
    let mut h = blake3::Hasher::new();
    let mut buf = vec![0u8; CHUNK];
    loop {
        if cancelled() {
            return Ok(None);
        }
        let n = f.read.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
        done.fetch_add(n as u64, Ordering::Relaxed);
        tick();
    }
    Ok(Some(hex(&h.finalize())))
}

/// Run `work(index)` for every item, one queue per physical disk.
fn run_pool(
    items: &[usize],
    cands: &[Candidate],
    env: &EngineEnv<'_>,
    work: &(dyn Fn(usize) + Sync),
    on_panic: &(dyn Fn(usize) + Sync),
) {
    let mut per_disk: HashMap<&str, (bool, Vec<usize>)> = HashMap::new();
    for &i in items {
        let e = per_disk.entry(cands[i].disk.as_str()).or_insert((cands[i].hdd, Vec::new()));
        e.0 &= cands[i].hdd;
        e.1.push(i);
    }
    let mut queues: Vec<(usize, Vec<usize>, AtomicUsize)> = per_disk
        .into_values()
        .map(|(hdd, mut list)| {
            // On-disk locality is approximated by snapshot order.
            list.sort_by_key(|&i| (cands[i].snapshot_id, cands[i].entry_id));
            (if hdd { 1 } else { env.ssd_readers.clamp(1, 16) }, list, AtomicUsize::new(0))
        })
        .collect();
    queues.shrink_to_fit();
    std::thread::scope(|s| {
        for (readers, list, next) in &queues {
            for _ in 0..*readers {
                s.spawn(move || loop {
                    if (env.cancelled)() {
                        break;
                    }
                    let n = next.fetch_add(1, Ordering::Relaxed);
                    let Some(&i) = list.get(n) else { break };
                    if catch_unwind(AssertUnwindSafe(|| work(i))).is_err() {
                        on_panic(i);
                    }
                });
            }
        }
    });
}

/// Find duplicate groups among `cands`. Candidates must already be filtered
/// (see [`super::select`]); only sizes occurring more than once are read.
pub fn find_duplicates(cands: &[Candidate], env: &EngineEnv<'_>) -> Result<Outcome, Cancelled> {
    let n = cands.len();
    let state =
        Mutex::new(State { partial: vec![None; n], full: vec![None; n], ids: vec![None; n], skipped: Vec::new() });
    let done = AtomicU64::new(0);
    let planned = AtomicU64::new(0);
    let (cache_hits, files_hashed) = (AtomicU64::new(0), AtomicU64::new(0));

    let mut by_size: HashMap<u64, Vec<usize>> = HashMap::new();
    for (i, c) in cands.iter().enumerate() {
        by_size.entry(c.size).or_default().push(i);
    }
    by_size.retain(|_, v| v.len() > 1);
    let stage_a: Vec<usize> = by_size.values().flatten().copied().collect();
    planned.fetch_add(stage_a.iter().map(|&i| cands[i].size.min(2 * PARTIAL_BYTES)).sum::<u64>(), Ordering::Relaxed);

    let report = || (env.progress)(done.load(Ordering::Relaxed), planned.load(Ordering::Relaxed));
    let skip = |i: usize, reason: SkipReason| {
        if let Ok(mut s) = state.lock() {
            s.skipped.push(Skip { candidate: i, reason });
        }
    };

    // Stage 2: partial hashes.
    let part = |i: usize| {
        let c = &cands[i];
        let cost = c.size.min(2 * PARTIAL_BYTES);
        let mut f = match open_checked(env, c) {
            Ok(f) => f,
            Err(r) => {
                done.fetch_add(cost, Ordering::Relaxed);
                skip(i, r);
                report();
                return;
            }
        };
        if let Some(id) = f.id {
            if let Ok(mut s) = state.lock() {
                s.ids[i] = Some(id);
            }
            let key = CacheKey { serial: id.serial, index: id.index, size: c.size, mtime: c.mtime };
            if let Some(p) = env.cache.get(&key).and_then(|h| h.partial) {
                cache_hits.fetch_add(1, Ordering::Relaxed);
                if let Ok(mut s) = state.lock() {
                    s.partial[i] = Some(p);
                }
                done.fetch_add(cost, Ordering::Relaxed);
                report();
                return;
            }
        }
        match partial_hash(&mut f) {
            Ok(p) => {
                if let Some(id) = f.id {
                    let key = CacheKey { serial: id.serial, index: id.index, size: c.size, mtime: c.mtime };
                    env.cache.put(&key, &CachedHash { partial: Some(p.clone()), full: None });
                }
                if let Ok(mut s) = state.lock() {
                    s.partial[i] = Some(p);
                }
            }
            Err(_) => skip(i, SkipReason::Error),
        }
        files_hashed.fetch_add(1, Ordering::Relaxed);
        done.fetch_add(cost, Ordering::Relaxed);
        report();
    };
    run_pool(&stage_a, cands, env, &part, &|i| skip(i, SkipReason::Error));
    if (env.cancelled)() {
        return Err(Cancelled);
    }

    // Collapse names of one file (same volume serial and file id) within a size group.
    let mut alias: Vec<bool> = vec![false; n];
    {
        let s = state.lock().map_err(|_| Cancelled)?;
        for members in by_size.values() {
            let mut seen: HashMap<FileId, usize> = HashMap::new();
            let mut sorted = members.clone();
            sorted.sort_unstable();
            for i in sorted {
                if let Some(id) = s.ids[i] {
                    if seen.insert(id, i).is_some() {
                        alias[i] = true;
                    }
                }
            }
        }
    }
    for (i, a) in alias.iter().enumerate() {
        if *a {
            skip(i, SkipReason::HardLink);
        }
    }

    // Partial-hash collisions go on to the full hash.
    let mut by_partial: HashMap<(u64, String), Vec<usize>> = HashMap::new();
    {
        let s = state.lock().map_err(|_| Cancelled)?;
        for members in by_size.values() {
            for &i in members {
                if alias[i] {
                    continue;
                }
                if let Some(p) = &s.partial[i] {
                    by_partial.entry((cands[i].size, p.clone())).or_default().push(i);
                }
            }
        }
    }
    by_partial.retain(|_, v| v.len() > 1);
    let stage_b: Vec<usize> = by_partial.values().flatten().copied().collect();
    planned.fetch_add(stage_b.iter().map(|&i| cands[i].size).sum::<u64>(), Ordering::Relaxed);
    report();

    // Stage 3: full hashes.
    let full = |i: usize| {
        let c = &cands[i];
        let id = state.lock().ok().and_then(|s| s.ids[i]);
        let key = id.map(|id| CacheKey { serial: id.serial, index: id.index, size: c.size, mtime: c.mtime });
        if let Some(h) = key.as_ref().and_then(|k| env.cache.get(k)).and_then(|h| h.full) {
            cache_hits.fetch_add(1, Ordering::Relaxed);
            if let Ok(mut s) = state.lock() {
                s.full[i] = Some(h);
            }
            done.fetch_add(c.size, Ordering::Relaxed);
            report();
            return;
        }
        let mut f = match open_checked(env, c) {
            Ok(f) => f,
            Err(r) => {
                done.fetch_add(c.size, Ordering::Relaxed);
                skip(i, r);
                report();
                return;
            }
        };
        let before = done.load(Ordering::Relaxed);
        match full_hash(&mut f, env.cancelled, &done, &report) {
            Ok(Some(h)) => {
                if let Some(k) = &key {
                    env.cache.put(k, &CachedHash { partial: None, full: Some(h.clone()) });
                }
                if let Ok(mut s) = state.lock() {
                    s.full[i] = Some(h);
                }
                files_hashed.fetch_add(1, Ordering::Relaxed);
            }
            Ok(None) => {} // cancelled
            Err(_) => {
                done.store(before + c.size, Ordering::Relaxed);
                skip(i, SkipReason::Error);
            }
        }
        report();
    };
    run_pool(&stage_b, cands, env, &full, &|i| skip(i, SkipReason::Error));
    if (env.cancelled)() {
        return Err(Cancelled);
    }

    let s = state.into_inner().map_err(|_| Cancelled)?;
    let mut by_full: HashMap<(u64, String), Vec<usize>> = HashMap::new();
    for members in by_partial.values() {
        for &i in members {
            if let Some(h) = &s.full[i] {
                by_full.entry((cands[i].size, h.clone())).or_default().push(i);
            }
        }
    }
    let mut groups: Vec<Group> = by_full
        .into_iter()
        .filter(|(_, v)| v.len() > 1)
        .map(|((size, hash), mut members)| {
            members.sort_unstable();
            Group { size, hash, members }
        })
        .collect();
    groups.sort_by(|a, b| {
        (b.size * (b.members.len() as u64 - 1)).cmp(&(a.size * (a.members.len() as u64 - 1))).then(a.hash.cmp(&b.hash))
    });
    Ok(Outcome {
        groups,
        skipped: s.skipped,
        bytes_read: done.load(Ordering::Relaxed),
        cache_hits: cache_hits.load(Ordering::Relaxed),
        files_hashed: files_hashed.load(Ordering::Relaxed),
    })
}

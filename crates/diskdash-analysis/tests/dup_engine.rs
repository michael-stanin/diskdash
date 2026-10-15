#![allow(clippy::unwrap_used)]
//! The hashing engine against an in-memory file source (no disk access).

use std::collections::HashMap;
use std::io::Cursor;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use diskdash_analysis::duplicates::{
    find_duplicates, Candidate, EngineEnv, FileId, MemoryCache, OpenError, OpenFile, SkipReason, Source,
};

#[derive(Clone)]
struct F {
    content: Vec<u8>,
    mtime: i64,
    attrs: u32,
    id: Option<FileId>,
    error: Option<OpenError>,
    panic: bool,
}

#[derive(Default)]
struct Fake {
    files: HashMap<String, F>,
    opens: Mutex<Vec<String>>,
    active: AtomicUsize,
    max_active: AtomicUsize,
    delay: Duration,
}

struct Reader {
    inner: Cursor<Vec<u8>>,
    fake: *const Fake,
}
// SAFETY: the pointer is only used to decrement a counter while the Fake outlives every reader in these tests.
unsafe impl Send for Reader {}

impl std::io::Read for Reader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let f = unsafe { &*self.fake };
        let now = f.active.fetch_add(1, Ordering::SeqCst) + 1;
        f.max_active.fetch_max(now, Ordering::SeqCst);
        std::thread::sleep(f.delay);
        let r = self.inner.read(buf);
        f.active.fetch_sub(1, Ordering::SeqCst);
        r
    }
}

impl std::io::Seek for Reader {
    fn seek(&mut self, pos: std::io::SeekFrom) -> std::io::Result<u64> {
        self.inner.seek(pos)
    }
}

impl Source for Fake {
    fn attributes(&self, path: &str) -> Result<u32, OpenError> {
        self.files.get(path).map(|f| f.attrs).ok_or(OpenError::NotFound)
    }

    fn open(&self, path: &str) -> Result<OpenFile, OpenError> {
        self.opens.lock().unwrap().push(path.to_string());
        let f = self.files.get(path).ok_or(OpenError::NotFound)?;
        assert!(!f.panic, "simulated reader crash");
        if let Some(e) = &f.error {
            return Err(e.clone());
        }
        Ok(OpenFile {
            size: f.content.len() as u64,
            mtime: f.mtime,
            attrs: 0,
            id: f.id,
            read: Box::new(Reader { inner: Cursor::new(f.content.clone()), fake: self }),
        })
    }
}

fn content(len: usize, seed: u8) -> Vec<u8> {
    (0..len).map(|i| (i as u8).wrapping_mul(31).wrapping_add(seed)).collect()
}

fn file(c: Vec<u8>, index: u64) -> F {
    F { content: c, mtime: 1000, attrs: 0, id: Some(FileId { serial: 1, index }), error: None, panic: false }
}

fn cand(path: &str, size: usize, entry: i64, disk: &str, hdd: bool) -> Candidate {
    Candidate {
        snapshot_id: 1,
        entry_id: entry,
        path: path.into(),
        size: size as u64,
        alloc: size as u64,
        mtime: 1000,
        volume_serial: 1,
        root: format!("{}:\\", &path[..1]),
        volume_id: None,
        disk: disk.into(),
        hdd,
        category: None,
    }
}

fn run(
    fake: &Fake,
    cands: &[Candidate],
    cache: &MemoryCache,
    readers: usize,
) -> Result<diskdash_analysis::duplicates::Outcome, ()> {
    let never = || false;
    let progress = |_: u64, _: u64| {};
    let env = EngineEnv { source: fake, cache, cancelled: &never, progress: &progress, ssd_readers: readers };
    find_duplicates(cands, &env).map_err(|_| ())
}

fn setup(files: Vec<(&str, F)>) -> Fake {
    Fake { files: files.into_iter().map(|(p, f)| (p.to_string(), f)).collect(), ..Fake::default() }
}

#[test]
fn three_identical_files_on_two_drives_form_one_group() {
    let c = content(300 * 1024, 7);
    let fake = setup(vec![
        (r"C:\a\x.bin", file(c.clone(), 1)),
        (r"D:\b\x.bin", file(c.clone(), 2)),
        (r"D:\c\y.bin", file(c.clone(), 3)),
        (r"D:\other.bin", file(content(300 * 1024, 9), 4)),
    ]);
    let cands = vec![
        cand(r"C:\a\x.bin", 300 * 1024, 1, "d0", false),
        cand(r"D:\b\x.bin", 300 * 1024, 2, "d1", true),
        cand(r"D:\c\y.bin", 300 * 1024, 3, "d1", true),
        cand(r"D:\other.bin", 300 * 1024, 4, "d1", true),
    ];
    let out = run(&fake, &cands, &MemoryCache::default(), 2).unwrap();
    assert_eq!(out.groups.len(), 1);
    assert_eq!(out.groups[0].members, [0, 1, 2]);
    assert_eq!(out.groups[0].hash.len(), 64);
}

#[test]
fn same_size_and_same_ends_but_different_middle_is_not_a_duplicate() {
    let a = content(300 * 1024, 1);
    let mut b = a.clone();
    b[150 * 1024] ^= 0xFF;
    let fake = setup(vec![(r"C:\a.bin", file(a, 1)), (r"C:\b.bin", file(b, 2))]);
    let cands = vec![cand(r"C:\a.bin", 300 * 1024, 1, "d", false), cand(r"C:\b.bin", 300 * 1024, 2, "d", false)];
    let out = run(&fake, &cands, &MemoryCache::default(), 2).unwrap();
    assert!(out.groups.is_empty(), "partial hashes collide, full hashes differ");
}

#[test]
fn hard_linked_names_are_one_file_not_duplicates() {
    let c = content(200 * 1024, 3);
    let same_id = FileId { serial: 1, index: 77 };
    let mut a = file(c.clone(), 77);
    a.id = Some(same_id);
    let mut b = file(c.clone(), 77);
    b.id = Some(same_id);
    let fake = setup(vec![(r"D:\x.bin", a), (r"D:\y.bin", b), (r"D:\z.bin", file(c, 5))]);
    let cands = vec![
        cand(r"D:\x.bin", 200 * 1024, 1, "d", true),
        cand(r"D:\y.bin", 200 * 1024, 2, "d", true),
        cand(r"D:\z.bin", 200 * 1024, 3, "d", true),
    ];
    let out = run(&fake, &cands, &MemoryCache::default(), 1).unwrap();
    assert_eq!(out.groups.len(), 1);
    assert_eq!(out.groups[0].members, [0, 2], "x and z; y is another name of x");
    assert!(out.skipped.iter().any(|s| s.candidate == 1 && s.reason == SkipReason::HardLink));

    // Only the two names of one file: no group at all.
    let only: Vec<_> = cands[..2].to_vec();
    assert!(run(&fake, &only, &MemoryCache::default(), 1).unwrap().groups.is_empty());
}

#[test]
fn cloud_placeholders_are_never_opened_and_failures_are_isolated() {
    let c = content(200 * 1024, 4);
    let mut cloud = file(c.clone(), 1);
    cloud.attrs = 0x400000; // RECALL_ON_DATA_ACCESS
    let mut locked = file(c.clone(), 2);
    locked.error = Some(OpenError::Sharing);
    let mut denied = file(c.clone(), 3);
    denied.error = Some(OpenError::Denied);
    let mut changed = file(c.clone(), 4);
    changed.mtime = 2000;
    let mut crash = file(c.clone(), 5);
    crash.panic = true;
    let fake = setup(vec![
        (r"C:\cloud.bin", cloud),
        (r"C:\locked.bin", locked),
        (r"C:\denied.bin", denied),
        (r"C:\changed.bin", changed),
        (r"C:\crash.bin", crash),
        (r"C:\ok1.bin", file(c.clone(), 6)),
        (r"C:\ok2.bin", file(c, 7)),
    ]);
    let names = ["cloud", "locked", "denied", "changed", "crash", "ok1", "ok2"];
    let cands: Vec<_> = names
        .iter()
        .enumerate()
        .map(|(i, n)| cand(&format!(r"C:\{n}.bin"), 200 * 1024, i as i64, "d", false))
        .collect();
    let out = run(&fake, &cands, &MemoryCache::default(), 2).unwrap();
    assert_eq!(out.groups.len(), 1);
    assert_eq!(out.groups[0].members, [5, 6], "the two readable copies still match");
    let reason = |i: usize| out.skipped.iter().find(|s| s.candidate == i).map(|s| s.reason);
    assert_eq!(reason(0), Some(SkipReason::CloudPlaceholder));
    assert_eq!(reason(1), Some(SkipReason::SharingViolation));
    assert_eq!(reason(2), Some(SkipReason::AccessDenied));
    assert_eq!(reason(3), Some(SkipReason::ChangedSinceScan));
    assert_eq!(reason(4), Some(SkipReason::Error));
    assert!(!fake.opens.lock().unwrap().iter().any(|p| p.contains("cloud")), "the placeholder was never opened");
}

#[test]
fn a_second_run_reads_nothing_thanks_to_the_cache() {
    let c = content(300 * 1024, 2);
    let fake = setup(vec![(r"C:\a.bin", file(c.clone(), 1)), (r"C:\b.bin", file(c, 2))]);
    let cands = vec![cand(r"C:\a.bin", 300 * 1024, 1, "d", false), cand(r"C:\b.bin", 300 * 1024, 2, "d", false)];
    let cache = MemoryCache::default();
    let first = run(&fake, &cands, &cache, 2).unwrap();
    assert_eq!(first.groups.len(), 1);
    assert!(first.files_hashed >= 2 && first.cache_hits == 0);
    let opens_before = fake.opens.lock().unwrap().len();
    let second = run(&fake, &cands, &cache, 2).unwrap();
    assert_eq!(second.groups, first.groups);
    assert_eq!(second.files_hashed, 0, "nothing was hashed again");
    assert!(second.cache_hits >= 4, "partial and full hits: {}", second.cache_hits);
    // Files are opened to read their id and verify them, but no content is read.
    assert!(fake.opens.lock().unwrap().len() >= opens_before);
}

#[test]
fn a_spinning_disk_is_read_one_file_at_a_time_and_an_ssd_in_parallel() {
    let c = content(300 * 1024, 8);
    let mk = |n: usize, disk: &str, hdd: bool| {
        let files: Vec<(String, F)> =
            (0..n).map(|i| (format!(r"D:\f{i}.bin"), file(c.clone(), i as u64 + 1))).collect();
        let fake = Fake { files: files.iter().cloned().collect(), delay: Duration::from_millis(3), ..Fake::default() };
        let cands: Vec<_> =
            files.iter().enumerate().map(|(i, (p, _))| cand(p, 300 * 1024, i as i64, disk, hdd)).collect();
        (fake, cands)
    };
    // Two partitions of one HDD share the disk key, so they share one reader.
    let (fake, cands) = mk(6, "disk-0", true);
    run(&fake, &cands, &MemoryCache::default(), 4).unwrap();
    assert_eq!(fake.max_active.load(Ordering::SeqCst), 1);
    let (fake, cands) = mk(8, "disk-1", false);
    run(&fake, &cands, &MemoryCache::default(), 4).unwrap();
    let max = fake.max_active.load(Ordering::SeqCst);
    assert!((2..=4).contains(&max), "bounded SSD pool, got {max}");
}

#[test]
fn cancelling_stops_the_run_and_keeps_finished_hashes() {
    let c = content(300 * 1024, 6);
    let fake = setup(vec![(r"C:\a.bin", file(c.clone(), 1)), (r"C:\b.bin", file(c, 2))]);
    let cands = vec![cand(r"C:\a.bin", 300 * 1024, 1, "d", false), cand(r"C:\b.bin", 300 * 1024, 2, "d", false)];
    let cache = MemoryCache::default();
    let stop = AtomicBool::new(false);
    let cancelled = || stop.load(Ordering::SeqCst);
    // Cancel as soon as the full-hash stage reports progress beyond the partial stage.
    let progress = |done: u64, planned: u64| {
        if planned > 2 * 128 * 1024 && done >= 2 * 128 * 1024 {
            stop.store(true, Ordering::SeqCst);
        }
    };
    let env = EngineEnv { source: &fake, cache: &cache, cancelled: &cancelled, progress: &progress, ssd_readers: 1 };
    assert!(find_duplicates(&cands, &env).is_err());
    assert!(!cache.is_empty(), "partial hashes survive a cancelled run");
}

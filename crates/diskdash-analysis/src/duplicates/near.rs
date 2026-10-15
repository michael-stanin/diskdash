//! Optional near-duplicate folders: folders whose sets of (name, size, hash)
//! overlap by at least a Jaccard threshold. Built from the already hashed files.

use std::collections::{BTreeSet, HashMap};

#[derive(Debug, Clone)]
pub struct NearFile {
    pub dir: String,
    pub name: String,
    pub size: u64,
    pub hash: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct NearPair {
    pub dir_a: String,
    pub dir_b: String,
    pub similarity: f64,
    pub shared_files: u64,
    pub shared_bytes: u64,
    pub differing_bytes: u64,
    pub files_a: u64,
    pub files_b: u64,
}

#[derive(Debug, Default)]
pub struct NearResult {
    pub pairs: Vec<NearPair>,
    /// More candidate pairs existed than the cap allows.
    pub truncated: bool,
}

type Entry = (String, u64, String);

/// Folders with at least `min_files` hashed files whose similarity is at least `threshold`.
pub fn near_duplicates(files: &[NearFile], threshold: f64, min_files: usize, max_pairs: usize) -> NearResult {
    let mut dirs: HashMap<String, BTreeSet<Entry>> = HashMap::new();
    for f in files {
        dirs.entry(f.dir.to_lowercase()).or_default().insert((f.name.to_lowercase(), f.size, f.hash.clone()));
    }
    dirs.retain(|_, s| s.len() >= min_files);
    let names: Vec<&String> = dirs.keys().collect();
    let index: HashMap<&String, usize> = names.iter().enumerate().map(|(i, d)| (*d, i)).collect();

    // Inverted index: which folders hold each (name, size, hash)?
    let mut inverted: HashMap<&Entry, Vec<usize>> = HashMap::new();
    for (d, set) in &dirs {
        for e in set {
            inverted.entry(e).or_default().push(index[d]);
        }
    }
    let mut shared: HashMap<(usize, usize), (u64, u64)> = HashMap::new();
    let mut truncated = false;
    'outer: for (e, ds) in &inverted {
        for (k, &a) in ds.iter().enumerate() {
            for &b in &ds[k + 1..] {
                let key = if a < b { (a, b) } else { (b, a) };
                if !shared.contains_key(&key) && shared.len() >= max_pairs {
                    truncated = true;
                    break 'outer;
                }
                let s = shared.entry(key).or_insert((0, 0));
                s.0 += 1;
                s.1 += e.1;
            }
        }
    }
    let total = |d: &String| -> u64 { dirs[d].iter().map(|e| e.1).sum() };
    let mut pairs: Vec<NearPair> = shared
        .into_iter()
        .filter_map(|((a, b), (n, bytes))| {
            let (da, db) = (names[a], names[b]);
            let (fa, fb) = (dirs[da].len() as u64, dirs[db].len() as u64);
            let sim = n as f64 / (fa + fb - n) as f64;
            (sim >= threshold).then(|| NearPair {
                dir_a: da.clone(),
                dir_b: db.clone(),
                similarity: sim,
                shared_files: n,
                shared_bytes: bytes,
                differing_bytes: (total(da) + total(db)).saturating_sub(2 * bytes),
                files_a: fa,
                files_b: fb,
            })
        })
        .collect();
    pairs.sort_by(|x, y| {
        y.similarity.total_cmp(&x.similarity).then(y.shared_bytes.cmp(&x.shared_bytes)).then(x.dir_a.cmp(&y.dir_a))
    });
    NearResult { pairs, truncated }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(dir: &str, n: usize, skip: Option<usize>) -> Vec<NearFile> {
        (0..n)
            .filter(|i| Some(*i) != skip)
            .map(|i| NearFile {
                dir: dir.into(),
                name: format!("f{i}.bin"),
                size: 1000 + i as u64,
                hash: format!("h{i}"),
            })
            .collect()
    }

    #[test]
    fn copied_folder_with_one_difference_is_near_duplicate() {
        let mut files = folder(r"D:\proj", 20, None);
        files.extend(folder(r"E:\backup\proj", 20, Some(3)));
        for mut f in folder(r"C:\other", 12, None) {
            f.hash = format!("x{}", f.name);
            files.push(f);
        }
        let r = near_duplicates(&files, 0.9, 10, 10_000);
        assert_eq!(r.pairs.len(), 1);
        let p = &r.pairs[0];
        assert!((p.similarity - 19.0 / 20.0).abs() < 1e-9, "{}", p.similarity);
        assert_eq!((p.shared_files, p.files_a.max(p.files_b)), (19, 20));
        assert!(p.differing_bytes > 0);
        assert!(!r.truncated);
    }

    #[test]
    fn threshold_min_files_and_pair_cap() {
        let mut files = folder(r"D:\a", 20, None);
        files.extend(folder(r"D:\b", 20, None).into_iter().filter(|f| f.name.as_str() < "f5"));
        assert!(near_duplicates(&files, 0.9, 10, 100).pairs.is_empty(), "b has too few files");
        let mut many = Vec::new();
        for d in 0..6 {
            many.extend(folder(&format!(r"D:\d{d}"), 10, None));
        }
        let r = near_duplicates(&many, 0.9, 10, 3);
        assert!(r.truncated);
        assert!(r.pairs.len() <= 3);
    }
}

//! Turn parsed records into entries, joining base records with their extension
//! records (files whose attributes did not fit in one record).

use std::collections::{HashMap, HashSet};

use super::model::{EntryName, MftEntry};
use super::record::{win_attrs, RawData, RawRecord};

struct Waiting {
    base: RawRecord,
    missing: HashSet<u64>,
    exts: Vec<RawRecord>,
}

#[derive(Default)]
pub struct Assembler {
    waiting: HashMap<u64, Waiting>,
    /// Extension records that arrived before their base record.
    early: HashMap<u64, Vec<(u64, RawRecord)>>,
    /// Bases emitted without all of their extension records.
    pub incomplete: u64,
}

impl Assembler {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed the record stored in MFT slot `index`.
    pub fn push(&mut self, index: u64, rec: RawRecord, out: &mut dyn FnMut(MftEntry)) {
        if !rec.in_use() {
            return;
        }
        if rec.is_extension() {
            let base = rec.base;
            if let Some(w) = self.waiting.get_mut(&base) {
                w.missing.remove(&index);
                w.exts.push(rec);
                if w.missing.is_empty() {
                    if let Some(w) = self.waiting.remove(&base) {
                        emit(base, w.base, &w.exts, out);
                    }
                }
            } else {
                self.early.entry(base).or_default().push((index, rec));
            }
            return;
        }
        let mut missing: HashSet<u64> = rec.attr_list.iter().map(|e| e.record).filter(|&r| r != index).collect();
        // Extension records seen before this base count as found.
        let mut exts = Vec::new();
        for (slot, e) in self.early.remove(&index).unwrap_or_default() {
            missing.remove(&slot);
            exts.push(e);
        }
        if missing.is_empty() {
            emit(index, rec, &exts, out);
        } else {
            self.waiting.insert(index, Waiting { base: rec, missing, exts });
        }
    }

    /// End of the stream: emit what is left, noting bases that never got their extensions.
    pub fn finish(mut self, out: &mut dyn FnMut(MftEntry)) {
        let mut keys: Vec<u64> = self.waiting.keys().copied().collect();
        keys.sort_unstable();
        for k in keys {
            if let Some(w) = self.waiting.remove(&k) {
                self.incomplete += 1;
                emit(k, w.base, &w.exts, out);
            }
        }
    }
}

fn emit(frn: u64, base: RawRecord, exts: &[RawRecord], out: &mut dyn FnMut(MftEntry)) {
    if let Some(e) = to_entry(frn, &base, exts) {
        out(e);
    }
}

/// Build the entry for a base record and its extensions; `None` when it has no usable name.
pub fn to_entry(frn: u64, base: &RawRecord, exts: &[RawRecord]) -> Option<MftEntry> {
    let mut names: Vec<EntryName> = Vec::new();
    for n in base.names.iter().chain(exts.iter().flat_map(|e| e.names.iter())) {
        if n.is_dos_only() {
            continue;
        }
        if !names.iter().any(|x| x.parent == n.parent && x.name == n.name) {
            names.push(EntryName { parent: n.parent, parent_seq: n.parent_seq, name: n.name.clone() });
        }
    }
    if names.is_empty() {
        return None;
    }
    let std = base.std.unwrap_or_default();
    let tag = base.reparse_tag.or_else(|| exts.iter().find_map(|e| e.reparse_tag)).unwrap_or(0);
    // The extent that starts at VCN 0 carries the stream's sizes.
    let data: Option<RawData> =
        base.data.iter().chain(exts.iter().flat_map(|e| e.data.iter())).find(|d| d.start_vcn == 0).copied();
    let is_dir = base.is_dir();
    let (size, allocated, resident, packed) = match (is_dir, data) {
        (true, _) | (_, None) => (0, 0, true, false),
        (false, Some(d)) => (d.size, d.allocated, d.resident, d.compressed_or_sparse),
    };
    Some(MftEntry {
        frn,
        seq: base.seq,
        is_dir,
        attrs: win_attrs(base),
        ctime: std.ctime,
        mtime: std.mtime,
        atime: std.atime,
        names,
        size,
        allocated,
        resident,
        compressed_or_sparse: packed,
        reparse_tag: tag,
    })
}

#[cfg(test)]
mod tests {
    use super::super::record::testutil::*;
    use super::super::record::{parse_record, FLAG_DIRECTORY, FLAG_IN_USE};
    use super::*;

    fn parsed(bytes: Vec<u8>) -> RawRecord {
        let mut b = bytes;
        parse_record(&mut b).unwrap()
    }

    fn collect(records: Vec<(u64, RawRecord)>) -> (Vec<MftEntry>, u64) {
        let mut a = Assembler::new();
        let mut out = Vec::new();
        for (i, r) in records {
            a.push(i, r, &mut |e| out.push(e));
        }
        let incomplete = {
            let mut tmp = Vec::new();
            let n = a.waiting.len() as u64;
            a.finish(&mut |e| tmp.push(e));
            out.extend(tmp);
            n
        };
        (out, incomplete)
    }

    #[test]
    fn a_plain_file_and_a_folder() {
        let f = parsed(record(
            1,
            FLAG_IN_USE,
            0,
            &[std_info(1, 2, 3, 0x20), file_name(5, 5, 1, "a.txt"), data_resident(10)],
        ));
        let d = parsed(record(1, FLAG_IN_USE | FLAG_DIRECTORY, 0, &[std_info(1, 2, 3, 0), file_name(5, 5, 1, "docs")]));
        let (v, inc) = collect(vec![(30, f), (31, d)]);
        assert_eq!(inc, 0);
        assert_eq!(v[0].size, 10);
        assert!(v[0].resident);
        assert!(v[1].is_dir && v[1].attrs & 0x10 != 0 && v[1].size == 0);
    }

    #[test]
    fn dos_names_are_ignored_and_hard_links_kept() {
        let r = parsed(record(
            1,
            FLAG_IN_USE,
            0,
            &[
                std_info(0, 0, 0, 0),
                file_name(5, 5, 1, "LongName.txt"),
                file_name(5, 5, 2, "LONGNA~1.TXT"),
                file_name(40, 1, 1, "link.txt"),
                data_resident(1),
            ],
        ));
        let (v, _) = collect(vec![(30, r)]);
        assert_eq!(v[0].names.iter().map(|n| n.name.as_str()).collect::<Vec<_>>(), ["LongName.txt", "link.txt"]);
    }

    #[test]
    fn data_in_an_extension_record_is_joined_to_its_base_in_either_order() {
        let base = || {
            parsed(record(
                1,
                FLAG_IN_USE,
                0,
                &[std_info(0, 0, 0, 0), file_name(5, 5, 1, "big.bin"), attr_list(&[(0x80, 0, 41)])],
            ))
        };
        let ext = || {
            parsed(record(
                1,
                FLAG_IN_USE,
                30 | (1u64 << 48),
                &[data_nonresident(1 << 20, 1 << 20, None, 0, 0, &[0x11, 0x10, 0x20])],
            ))
        };

        let (v, inc) = collect(vec![(30, base()), (41, ext())]);
        assert_eq!((v.len(), inc, v[0].size), (1, 0, 1 << 20));
        let (v, inc) = collect(vec![(41, ext()), (30, base())]);
        assert_eq!((v.len(), inc, v[0].size, v[0].allocated), (1, 0, 1 << 20, 1 << 20), "extension before base");
    }

    #[test]
    fn a_missing_extension_is_reported_and_the_entry_still_appears() {
        let base = parsed(record(1, FLAG_IN_USE, 0, &[file_name(5, 5, 1, "big.bin"), attr_list(&[(0x80, 0, 99)])]));
        let (v, inc) = collect(vec![(30, base)]);
        assert_eq!((v.len(), inc), (1, 1));
        assert_eq!(v[0].size, 0);
    }

    #[test]
    fn unused_and_nameless_records_produce_nothing() {
        let deleted = parsed(record(1, 0, 0, &[file_name(5, 5, 1, "gone")]));
        let nameless = parsed(record(1, FLAG_IN_USE, 0, &[std_info(0, 0, 0, 0)]));
        let (v, _) = collect(vec![(30, deleted), (31, nameless)]);
        assert!(v.is_empty());
    }

    #[test]
    fn compressed_data_reports_its_on_disk_size() {
        let r = parsed(record(
            1,
            FLAG_IN_USE,
            0,
            &[file_name(5, 5, 1, "c.bin"), data_nonresident(1 << 20, 1 << 20, Some(70_000), 1, 0, &[0x11, 0x10, 0x20])],
        ));
        let (v, _) = collect(vec![(30, r)]);
        assert!(v[0].compressed_or_sparse);
        assert_eq!(v[0].allocated, 70_000);
    }
}

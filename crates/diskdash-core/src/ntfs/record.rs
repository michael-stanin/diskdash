//! One NTFS FILE record, parsed with explicit bounds checks.
//!
//! Every offset and length read from disk is validated against the record
//! buffer; a damaged record returns [`ParseError`], never a panic. There is no
//! `unsafe` here.

use super::runs::{self, Run};
use crate::win::filetime_to_unix;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ParseError {
    #[error("record is too small")]
    TooSmall,
    #[error("not a FILE record")]
    BadSignature,
    #[error("update sequence fixup does not match")]
    BadFixup,
    #[error("attribute header is out of bounds")]
    BadAttribute,
    #[error("data run list is damaged")]
    BadRuns,
    #[error("file name attribute is damaged")]
    BadName,
    #[error("attribute list is stored outside the record (unsupported)")]
    ExternalAttributeList,
}

pub const FLAG_IN_USE: u16 = 0x01;
pub const FLAG_DIRECTORY: u16 = 0x02;

const ATTR_STANDARD_INFORMATION: u32 = 0x10;
const ATTR_ATTRIBUTE_LIST: u32 = 0x20;
const ATTR_FILE_NAME: u32 = 0x30;
const ATTR_DATA: u32 = 0x80;
const ATTR_REPARSE_POINT: u32 = 0xC0;
const ATTR_END: u32 = 0xFFFF_FFFF;

const ATTR_FLAG_COMPRESSED: u16 = 0x0001;
const ATTR_FLAG_SPARSE: u16 = 0x8000;

/// Low 48 bits of a file reference: the MFT record number.
pub fn frn(reference: u64) -> u64 {
    reference & 0x0000_FFFF_FFFF_FFFF
}

/// High 16 bits of a file reference: the sequence number.
pub fn seq_of(reference: u64) -> u16 {
    (reference >> 48) as u16
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct StdInfo {
    pub ctime: i64,
    pub mtime: i64,
    pub atime: i64,
    /// Win32 file attributes (without the directory bit, which NTFS keeps in the record flags).
    pub attrs: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawName {
    pub parent: u64,
    pub parent_seq: u16,
    pub namespace: u8,
    pub name: String,
}

impl RawName {
    /// DOS-only 8.3 names are ignored: the file's long name is its real name.
    pub fn is_dos_only(&self) -> bool {
        self.namespace == 2
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RawData {
    pub size: u64,
    /// On-disk allocation: the attribute's allocated size, or its compressed size
    /// for compressed and sparse streams; 0 when the data is resident.
    pub allocated: u64,
    pub resident: bool,
    pub compressed_or_sparse: bool,
    /// Starting VCN of this extent (only the first extent carries sizes).
    pub start_vcn: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AttrListEntry {
    pub attr_type: u32,
    pub start_vcn: u64,
    /// Record number holding this attribute extent.
    pub record: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RawRecord {
    pub seq: u16,
    pub flags: u16,
    /// Record number of the base record, 0 when this is the base.
    pub base: u64,
    pub std: Option<StdInfo>,
    pub names: Vec<RawName>,
    /// Unnamed `$DATA` extents found in this record.
    pub data: Vec<RawData>,
    pub reparse_tag: Option<u32>,
    pub attr_list: Vec<AttrListEntry>,
    /// Run list of the first unnamed non-resident `$DATA` extent (used for `$MFT`).
    pub data_runs: Vec<Run>,
}

impl RawRecord {
    pub fn in_use(&self) -> bool {
        self.flags & FLAG_IN_USE != 0
    }

    pub fn is_dir(&self) -> bool {
        self.flags & FLAG_DIRECTORY != 0
    }

    pub fn is_extension(&self) -> bool {
        self.base != 0
    }
}

fn u16_at(b: &[u8], off: usize) -> Result<u16, ParseError> {
    let s = b.get(off..off.checked_add(2).ok_or(ParseError::BadAttribute)?).ok_or(ParseError::BadAttribute)?;
    Ok(u16::from_le_bytes([s[0], s[1]]))
}

fn u32_at(b: &[u8], off: usize) -> Result<u32, ParseError> {
    let s = b.get(off..off.checked_add(4).ok_or(ParseError::BadAttribute)?).ok_or(ParseError::BadAttribute)?;
    Ok(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

fn u64_at(b: &[u8], off: usize) -> Result<u64, ParseError> {
    let s = b.get(off..off.checked_add(8).ok_or(ParseError::BadAttribute)?).ok_or(ParseError::BadAttribute)?;
    let mut a = [0u8; 8];
    a.copy_from_slice(s);
    Ok(u64::from_le_bytes(a))
}

fn filetime_at(b: &[u8], off: usize) -> Result<i64, ParseError> {
    let v = u64_at(b, off)?;
    Ok(filetime_to_unix(v as u32, (v >> 32) as u32))
}

/// Undo the update-sequence protection in place: the last two bytes of every
/// 512-byte stripe were replaced by a sequence number on write.
fn apply_fixups(rec: &mut [u8]) -> Result<(), ParseError> {
    let usa_off = usize::from(u16_at(rec, 4)?);
    let usa_count = usize::from(u16_at(rec, 6)?);
    if usa_count < 2 || usa_off < 0x2A {
        return Err(ParseError::BadFixup);
    }
    let usa_end = usa_off.checked_add(usa_count * 2).ok_or(ParseError::BadFixup)?;
    if usa_end > rec.len() || (usa_count - 1) * 512 > rec.len() {
        return Err(ParseError::BadFixup);
    }
    let usn = [rec[usa_off], rec[usa_off + 1]];
    for i in 1..usa_count {
        let end = i * 512;
        if rec[end - 2] != usn[0] || rec[end - 1] != usn[1] {
            return Err(ParseError::BadFixup);
        }
        rec[end - 2] = rec[usa_off + i * 2];
        rec[end - 1] = rec[usa_off + i * 2 + 1];
    }
    Ok(())
}

/// Parse one FILE record. `rec` is modified in place (fixups are applied).
pub fn parse_record(rec: &mut [u8]) -> Result<RawRecord, ParseError> {
    if rec.len() < 0x30 {
        return Err(ParseError::TooSmall);
    }
    if &rec[0..4] != b"FILE" {
        return Err(ParseError::BadSignature);
    }
    apply_fixups(rec)?;
    let mut out = RawRecord {
        seq: u16_at(rec, 0x10)?,
        flags: u16_at(rec, 0x16)?,
        base: frn(u64_at(rec, 0x20)?),
        ..RawRecord::default()
    };
    // Deleted (not in use) records carry nothing we want, and their attributes may be stale.
    if !out.in_use() {
        return Ok(out);
    }
    let used = (u32_at(rec, 0x18)? as usize).min(rec.len());
    let mut off = usize::from(u16_at(rec, 0x14)?);
    if off < 0x30 {
        return Err(ParseError::BadAttribute);
    }
    let body = &rec[..used];
    // An attribute header is at least 16 bytes; a type of 0xFFFFFFFF ends the list.
    while let Ok(attr_type) = u32_at(body, off) {
        if attr_type == ATTR_END {
            break;
        }
        let len = u32_at(body, off + 4)? as usize;
        if len < 0x18 || !len.is_multiple_of(8) {
            return Err(ParseError::BadAttribute);
        }
        let end = off.checked_add(len).ok_or(ParseError::BadAttribute)?;
        let attr = body.get(off..end).ok_or(ParseError::BadAttribute)?;
        parse_attribute(attr_type, attr, &mut out)?;
        off = end;
    }
    Ok(out)
}

fn parse_attribute(attr_type: u32, a: &[u8], out: &mut RawRecord) -> Result<(), ParseError> {
    let non_resident = *a.get(8).ok_or(ParseError::BadAttribute)? != 0;
    let name_len = *a.get(9).ok_or(ParseError::BadAttribute)?;
    let flags = u16_at(a, 12)?;
    let named = name_len != 0;

    // Resident value, if any.
    let resident_value = || -> Result<&[u8], ParseError> {
        let len = u32_at(a, 0x10)? as usize;
        let off = usize::from(u16_at(a, 0x14)?);
        a.get(off..off.checked_add(len).ok_or(ParseError::BadAttribute)?).ok_or(ParseError::BadAttribute)
    };

    match attr_type {
        ATTR_STANDARD_INFORMATION if !non_resident => {
            let v = resident_value()?;
            out.std = Some(StdInfo {
                ctime: filetime_at(v, 0)?,
                mtime: filetime_at(v, 8)?,
                atime: filetime_at(v, 0x18)?,
                attrs: u32_at(v, 0x20)?,
            });
        }
        ATTR_FILE_NAME if !non_resident => {
            let v = resident_value()?;
            let parent_ref = u64_at(v, 0)?;
            let chars = usize::from(*v.get(0x40).ok_or(ParseError::BadName)?);
            let namespace = *v.get(0x41).ok_or(ParseError::BadName)?;
            let raw = v.get(0x42..0x42 + chars * 2).ok_or(ParseError::BadName)?;
            let units: Vec<u16> = raw.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect();
            out.names.push(RawName {
                parent: frn(parent_ref),
                parent_seq: seq_of(parent_ref),
                namespace,
                name: String::from_utf16_lossy(&units),
            });
        }
        ATTR_DATA if !named => {
            if non_resident {
                let start_vcn = u64_at(a, 0x10)?;
                let runs_off = usize::from(u16_at(a, 0x20)?);
                let unit = u16_at(a, 0x22)?;
                let allocated = u64_at(a, 0x28)?;
                let size = u64_at(a, 0x30)?;
                let packed = unit != 0 || flags & (ATTR_FLAG_COMPRESSED | ATTR_FLAG_SPARSE) != 0;
                // Only the first extent's header carries the totals.
                let on_disk = if packed && start_vcn == 0 { u64_at(a, 0x40).unwrap_or(allocated) } else { allocated };
                out.data.push(RawData {
                    size,
                    allocated: on_disk,
                    resident: false,
                    compressed_or_sparse: packed,
                    start_vcn,
                });
                if out.data_runs.is_empty() {
                    let run_bytes = a.get(runs_off..).ok_or(ParseError::BadRuns)?;
                    out.data_runs = runs::decode(run_bytes)?;
                }
            } else {
                let v = resident_value()?;
                out.data.push(RawData {
                    size: v.len() as u64,
                    allocated: 0,
                    resident: true,
                    compressed_or_sparse: false,
                    start_vcn: 0,
                });
            }
        }
        ATTR_REPARSE_POINT if !non_resident => {
            let v = resident_value()?;
            out.reparse_tag = Some(u32_at(v, 0)?);
        }
        ATTR_ATTRIBUTE_LIST => {
            if non_resident {
                return Err(ParseError::ExternalAttributeList);
            }
            let v = resident_value()?;
            let mut i = 0usize;
            while i + 0x1A <= v.len() {
                let rec_len = usize::from(u16_at(v, i + 4)?);
                if rec_len < 0x1A {
                    return Err(ParseError::BadAttribute);
                }
                out.attr_list.push(AttrListEntry {
                    attr_type: u32_at(v, i)?,
                    start_vcn: u64_at(v, i + 8)?,
                    record: frn(u64_at(v, i + 0x10)?),
                });
                i += rec_len;
            }
        }
        _ => {}
    }
    Ok(())
}

/// Convert a Win32-visible attribute set for a parsed record (adds the directory bit).
pub fn win_attrs(rec: &RawRecord) -> u32 {
    let base = rec.std.map_or(0, |s| s.attrs);
    if rec.is_dir() {
        base | crate::alloc::FILE_ATTRIBUTE_DIRECTORY
    } else {
        base
    }
}

#[cfg(test)]
pub(crate) mod testutil {
    //! Builds FILE records byte by byte, the way NTFS lays them out.

    pub const RECORD_SIZE: usize = 1024;

    pub fn filetime(unix: i64) -> u64 {
        ((unix * 10_000_000) + 116_444_736_000_000_000) as u64
    }

    fn attr_header(attr_type: u32, len: usize, non_resident: bool, name_len: u8, flags: u16) -> Vec<u8> {
        let mut h = vec![0u8; 0x18];
        h[0..4].copy_from_slice(&attr_type.to_le_bytes());
        h[4..8].copy_from_slice(&(len as u32).to_le_bytes());
        h[8] = u8::from(non_resident);
        h[9] = name_len;
        h[12..14].copy_from_slice(&flags.to_le_bytes());
        h
    }

    fn pad8(mut v: Vec<u8>) -> Vec<u8> {
        while !v.len().is_multiple_of(8) {
            v.push(0);
        }
        v
    }

    pub fn resident(attr_type: u32, value: &[u8]) -> Vec<u8> {
        let total = (0x18 + value.len()).div_ceil(8) * 8;
        let mut a = attr_header(attr_type, total, false, 0, 0);
        a[0x10..0x14].copy_from_slice(&(value.len() as u32).to_le_bytes());
        a[0x14..0x16].copy_from_slice(&0x18u16.to_le_bytes());
        a.extend_from_slice(value);
        pad8(a)
    }

    pub fn std_info(ctime: i64, mtime: i64, atime: i64, attrs: u32) -> Vec<u8> {
        let mut v = vec![0u8; 0x30];
        v[0..8].copy_from_slice(&filetime(ctime).to_le_bytes());
        v[8..16].copy_from_slice(&filetime(mtime).to_le_bytes());
        v[0x18..0x20].copy_from_slice(&filetime(atime).to_le_bytes());
        v[0x20..0x24].copy_from_slice(&attrs.to_le_bytes());
        resident(0x10, &v)
    }

    pub fn file_name(parent: u64, parent_seq: u16, namespace: u8, name: &str) -> Vec<u8> {
        let units: Vec<u16> = name.encode_utf16().collect();
        let mut v = vec![0u8; 0x42];
        v[0..8].copy_from_slice(&(parent | (u64::from(parent_seq) << 48)).to_le_bytes());
        v[0x40] = units.len() as u8;
        v[0x41] = namespace;
        for u in units {
            v.extend_from_slice(&u.to_le_bytes());
        }
        resident(0x30, &v)
    }

    pub fn data_resident(len: usize) -> Vec<u8> {
        resident(0x80, &vec![7u8; len])
    }

    /// Non-resident unnamed `$DATA`. `runs` is the raw packed run list.
    pub fn data_nonresident(
        size: u64,
        allocated: u64,
        compressed: Option<u64>,
        flags: u16,
        start_vcn: u64,
        runs: &[u8],
    ) -> Vec<u8> {
        let total = (0x48 + runs.len() + 1).div_ceil(8) * 8;
        let mut a = attr_header(0x80, total, true, 0, flags);
        a.resize(0x48, 0);
        a[0x10..0x18].copy_from_slice(&start_vcn.to_le_bytes());
        a[0x20..0x22].copy_from_slice(&0x48u16.to_le_bytes());
        if compressed.is_some() {
            a[0x22..0x24].copy_from_slice(&4u16.to_le_bytes());
        }
        a[0x28..0x30].copy_from_slice(&allocated.to_le_bytes());
        a[0x30..0x38].copy_from_slice(&size.to_le_bytes());
        a[0x38..0x40].copy_from_slice(&size.to_le_bytes());
        a[0x40..0x48].copy_from_slice(&compressed.unwrap_or(0).to_le_bytes());
        a.extend_from_slice(runs);
        a.push(0);
        pad8(a)
    }

    pub fn reparse(tag: u32) -> Vec<u8> {
        let mut v = tag.to_le_bytes().to_vec();
        v.extend_from_slice(&[0; 8]);
        resident(0xC0, &v)
    }

    pub fn attr_list(entries: &[(u32, u64, u64)]) -> Vec<u8> {
        let mut v = Vec::new();
        for &(t, vcn, rec) in entries {
            let mut e = vec![0u8; 0x20];
            e[0..4].copy_from_slice(&t.to_le_bytes());
            e[4..6].copy_from_slice(&0x20u16.to_le_bytes());
            e[8..16].copy_from_slice(&vcn.to_le_bytes());
            e[0x10..0x18].copy_from_slice(&rec.to_le_bytes());
            v.extend_from_slice(&e);
        }
        resident(0x20, &v)
    }

    /// Assemble a record with valid update-sequence protection.
    pub fn record(seq: u16, flags: u16, base: u64, attrs: &[Vec<u8>]) -> Vec<u8> {
        let mut r = vec![0u8; RECORD_SIZE];
        r[0..4].copy_from_slice(b"FILE");
        r[4..6].copy_from_slice(&0x30u16.to_le_bytes());
        r[6..8].copy_from_slice(&3u16.to_le_bytes()); // USN + two stripes
        r[0x10..0x12].copy_from_slice(&seq.to_le_bytes());
        r[0x14..0x16].copy_from_slice(&0x38u16.to_le_bytes());
        r[0x16..0x18].copy_from_slice(&flags.to_le_bytes());
        r[0x20..0x28].copy_from_slice(&base.to_le_bytes());
        let mut off = 0x38;
        for a in attrs {
            r[off..off + a.len()].copy_from_slice(a);
            off += a.len();
        }
        r[off..off + 4].copy_from_slice(&0xFFFF_FFFFu32.to_le_bytes());
        off += 8;
        r[0x18..0x1C].copy_from_slice(&(off as u32).to_le_bytes());
        r[0x1C..0x20].copy_from_slice(&(RECORD_SIZE as u32).to_le_bytes());
        // Fixups: save the last two bytes of each stripe in the array, then stamp the USN.
        let usn = [0x34u8, 0x12];
        r[0x30..0x32].copy_from_slice(&usn);
        for i in 1..3usize {
            let end = i * 512;
            let (a, b) = (r[end - 2], r[end - 1]);
            r[0x30 + i * 2] = a;
            r[0x30 + i * 2 + 1] = b;
            r[end - 2] = usn[0];
            r[end - 1] = usn[1];
        }
        r
    }
}

#[cfg(test)]
mod tests {
    use super::testutil::*;
    use super::*;

    fn file(attrs: &[Vec<u8>]) -> Vec<u8> {
        record(3, FLAG_IN_USE, 0, attrs)
    }

    #[test]
    fn parses_a_resident_file() {
        let mut r = file(&[std_info(1_000, 2_000, 3_000, 0x20), file_name(5, 5, 1, "notes.txt"), data_resident(100)]);
        let p = parse_record(&mut r).unwrap();
        assert!(p.in_use() && !p.is_dir());
        assert_eq!(p.seq, 3);
        assert_eq!(p.std, Some(StdInfo { ctime: 1_000, mtime: 2_000, atime: 3_000, attrs: 0x20 }));
        assert_eq!(p.names.len(), 1);
        assert_eq!((p.names[0].parent, p.names[0].parent_seq, p.names[0].name.as_str()), (5, 5, "notes.txt"));
        assert_eq!(
            p.data,
            vec![RawData { size: 100, allocated: 0, resident: true, compressed_or_sparse: false, start_vcn: 0 }]
        );
    }

    #[test]
    fn applies_fixups_so_data_after_a_stripe_boundary_is_intact() {
        // A long name pushes bytes across the first stripe end (offset 510).
        let long = "x".repeat(200);
        let mut r =
            file(&[std_info(1, 2, 3, 0), file_name(5, 5, 1, &long), file_name(5, 5, 2, "SHORT~1"), data_resident(10)]);
        let p = parse_record(&mut r).unwrap();
        assert_eq!(p.names[0].name, long);
        assert_eq!(p.names.len(), 2);
        assert!(p.names[1].is_dos_only());
    }

    #[test]
    fn non_resident_sizes_plain_compressed_and_sparse() {
        let plain = data_nonresident(10_000, 12_288, None, 0, 0, &[0x11, 0x03, 0x20]);
        let p = parse_record(&mut file(&[std_info(0, 0, 0, 0), file_name(5, 5, 1, "a.bin"), plain])).unwrap();
        assert_eq!(
            p.data[0],
            RawData { size: 10_000, allocated: 12_288, resident: false, compressed_or_sparse: false, start_vcn: 0 }
        );
        assert_eq!(p.data_runs[0], Run { lcn: Some(0x20), clusters: 3 });

        let comp = data_nonresident(1 << 20, 1 << 20, Some(70_000), ATTR_FLAG_COMPRESSED, 0, &[0x11, 0x10, 0x20]);
        let p = parse_record(&mut file(&[file_name(5, 5, 1, "c.bin"), comp])).unwrap();
        assert_eq!(p.data[0].allocated, 70_000, "compressed size is the real allocation");
        assert!(p.data[0].compressed_or_sparse);

        let sparse =
            data_nonresident(1 << 30, 1 << 30, Some(8192), ATTR_FLAG_SPARSE, 0, &[0x01, 0x02, 0x11, 0x02, 0x10]);
        let p = parse_record(&mut file(&[file_name(5, 5, 1, "s.bin"), sparse])).unwrap();
        assert_eq!(p.data[0].allocated, 8192);
    }

    #[test]
    fn all_hard_links_and_reparse_tag_are_reported() {
        let mut r = file(&[
            std_info(0, 0, 0, 0x400),
            file_name(10, 1, 1, "one.txt"),
            file_name(20, 2, 1, "two.txt"),
            reparse(0xA000_000C),
        ]);
        let p = parse_record(&mut r).unwrap();
        assert_eq!(
            p.names.iter().map(|n| (n.parent, n.name.as_str())).collect::<Vec<_>>(),
            [(10, "one.txt"), (20, "two.txt")]
        );
        assert_eq!(p.reparse_tag, Some(0xA000_000C));
    }

    #[test]
    fn directory_flag_and_extension_records() {
        let mut d = record(1, FLAG_IN_USE | FLAG_DIRECTORY, 0, &[std_info(0, 0, 0, 0), file_name(5, 5, 1, "docs")]);
        let p = parse_record(&mut d).unwrap();
        assert!(p.is_dir());
        assert_eq!(win_attrs(&p) & 0x10, 0x10);

        let mut ext =
            record(2, FLAG_IN_USE, 40 | (7u64 << 48), &[data_nonresident(5, 4096, None, 0, 0, &[0x11, 0x01, 0x30])]);
        let p = parse_record(&mut ext).unwrap();
        assert!(p.is_extension());
        assert_eq!(p.base, 40);

        let mut base = file(&[file_name(5, 5, 1, "big.bin"), attr_list(&[(0x80, 0, 41)])]);
        let p = parse_record(&mut base).unwrap();
        assert_eq!(p.attr_list, vec![AttrListEntry { attr_type: 0x80, start_vcn: 0, record: 41 }]);
    }

    #[test]
    fn deleted_records_are_empty() {
        let mut r = record(9, 0, 0, &[file_name(5, 5, 1, "gone.txt")]);
        let p = parse_record(&mut r).unwrap();
        assert!(!p.in_use());
        assert!(p.names.is_empty());
    }

    #[test]
    fn corrupt_records_are_errors_not_panics() {
        let good = file(&[std_info(0, 0, 0, 0), file_name(5, 5, 1, "a"), data_resident(4)]);

        let mut bad = good.clone();
        bad[0] = b'B';
        assert_eq!(parse_record(&mut bad), Err(ParseError::BadSignature));

        let mut bad = good.clone();
        bad[510] ^= 0xFF; // stripe end no longer matches the update sequence number
        assert_eq!(parse_record(&mut bad), Err(ParseError::BadFixup));

        let mut bad = good.clone();
        bad[0x38 + 4] = 0xF1; // attribute length not a multiple of 8
        assert!(parse_record(&mut bad).is_err());

        let mut bad = good.clone();
        bad[0x38 + 4..0x38 + 8].copy_from_slice(&0x00FF_FFF8u32.to_le_bytes()); // runs past the record
        assert_eq!(parse_record(&mut bad), Err(ParseError::BadAttribute));

        assert_eq!(parse_record(&mut [0u8; 10]), Err(ParseError::TooSmall));
        let mut bad = good;
        bad[6] = 0xFF; // absurd update sequence count
        assert_eq!(parse_record(&mut bad), Err(ParseError::BadFixup));
    }

    #[test]
    fn every_truncation_and_bit_flip_of_a_record_is_handled() {
        let good = file(&[
            std_info(1, 2, 3, 0x20),
            file_name(5, 5, 1, "fuzz.txt"),
            data_nonresident(5, 4096, None, 0, 0, &[0x11, 0x01, 0x30]),
            reparse(0x8000_0017),
        ]);
        for len in 0..good.len() {
            let mut cut = good[..len].to_vec();
            let _ = parse_record(&mut cut);
        }
        for i in 0..good.len() {
            for bit in [0x01u8, 0x80] {
                let mut flipped = good.clone();
                flipped[i] ^= bit;
                let _ = parse_record(&mut flipped);
            }
        }
    }
}

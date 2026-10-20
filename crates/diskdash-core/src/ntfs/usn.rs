//! USN change journal: parsing what Windows returns and deciding when a quick
//! rescan is safe. Reading only; the journal is never created, resized or deleted.

use serde::{Deserialize, Serialize};

/// A quick rescan is abandoned for a full scan when more than this share of records changed.
pub const FULL_SCAN_CHANGED_FRACTION: f64 = 0.20;

/// What `FSCTL_QUERY_USN_JOURNAL` reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct JournalState {
    pub journal_id: u64,
    pub first_usn: i64,
    pub next_usn: i64,
    /// False when the volume has no journal.
    pub active: bool,
}

/// What a scan stored for the volume, to resume from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredUsn {
    pub journal_id: u64,
    pub next_usn: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct UsnChange {
    /// MFT record number of the file that changed.
    pub frn: u64,
    pub usn: i64,
    pub reason: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FullReason {
    NoPriorState,
    JournalChanged,
    Wrapped,
    Inactive,
    TooManyChanges { changed: u64, total: u64 },
}

impl FullReason {
    /// Sentence for the log and the UI.
    pub fn message(&self) -> String {
        match self {
            Self::NoPriorState => "no earlier fast scan of this drive; running a full scan".into(),
            Self::JournalChanged => "the change journal was recreated; running a full scan".into(),
            Self::Wrapped => "USN journal wrapped; running a full scan".into(),
            Self::Inactive => "the change journal is not active on this drive; running a full scan".into(),
            Self::TooManyChanges { changed, total } => {
                format!("{changed} of {total} records changed (over 20%); running a full scan")
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Plan {
    Quick,
    Full(FullReason),
}

/// Can the journal be read from where the last scan stopped?
pub fn plan(stored: Option<StoredUsn>, now: &JournalState) -> Plan {
    let Some(s) = stored else { return Plan::Full(FullReason::NoPriorState) };
    if !now.active {
        return Plan::Full(FullReason::Inactive);
    }
    if s.journal_id != now.journal_id {
        return Plan::Full(FullReason::JournalChanged);
    }
    if s.next_usn < now.first_usn {
        return Plan::Full(FullReason::Wrapped);
    }
    Plan::Quick
}

/// After reading the journal: too many changed records make a full scan cheaper.
pub fn check_volume_of_changes(changed_records: u64, total_records: u64) -> Plan {
    if total_records > 0 && changed_records as f64 / total_records as f64 > FULL_SCAN_CHANGED_FRACTION {
        Plan::Full(FullReason::TooManyChanges { changed: changed_records, total: total_records })
    } else {
        Plan::Quick
    }
}

/// Parse one buffer returned by `FSCTL_READ_USN_JOURNAL`: the next USN to ask for,
/// then `USN_RECORD_V2/V3` records. Unknown record versions are skipped.
pub fn parse_read_buffer(b: &[u8]) -> Result<(i64, Vec<UsnChange>), String> {
    let next = i64::from_le_bytes(b.get(0..8).ok_or("USN buffer too short")?.try_into().map_err(|_| "bad buffer")?);
    let mut out = Vec::new();
    let mut i = 8usize;
    while i + 4 <= b.len() {
        let len = u32::from_le_bytes(b[i..i + 4].try_into().map_err(|_| "bad record")?) as usize;
        if len < 0x38 || !len.is_multiple_of(8) {
            return Err(format!("damaged USN record at offset {i}"));
        }
        let rec = b.get(i..i + len).ok_or("USN record runs past the buffer")?;
        let major = u16::from_le_bytes([rec[4], rec[5]]);
        let get_u64 =
            |o: usize| -> Option<u64> { rec.get(o..o + 8).and_then(|s| s.try_into().ok()).map(u64::from_le_bytes) };
        match major {
            2 => {
                // FileReferenceNumber @8, ParentFileReferenceNumber @16, Usn @24, Reason @40.
                let (frn, usn) = (get_u64(8).ok_or("short record")?, get_u64(24).ok_or("short record")? as i64);
                let reason = u32::from_le_bytes(rec.get(40..44).ok_or("short record")?.try_into().map_err(|_| "bad")?);
                out.push(UsnChange { frn: frn & 0x0000_FFFF_FFFF_FFFF, usn, reason });
            }
            3 => {
                // 128-bit references: FRN @8, parent @24, Usn @40, Reason @56.
                let (frn, usn) = (get_u64(8).ok_or("short record")?, get_u64(40).ok_or("short record")? as i64);
                let reason = u32::from_le_bytes(rec.get(56..60).ok_or("short record")?.try_into().map_err(|_| "bad")?);
                out.push(UsnChange { frn: frn & 0x0000_FFFF_FFFF_FFFF, usn, reason });
            }
            _ => {}
        }
        i += len;
    }
    Ok((next, out))
}

/// Distinct changed records, in record order.
pub fn distinct_frns(changes: &[UsnChange]) -> Vec<u64> {
    let mut v: Vec<u64> = changes.iter().map(|c| c.frn).collect();
    v.sort_unstable();
    v.dedup();
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: JournalState = JournalState { journal_id: 7, first_usn: 1000, next_usn: 5000, active: true };

    #[test]
    fn quick_rescan_needs_matching_journal_and_unwrapped_position() {
        assert_eq!(plan(Some(StoredUsn { journal_id: 7, next_usn: 3000 }), &NOW), Plan::Quick);
        assert_eq!(
            plan(Some(StoredUsn { journal_id: 7, next_usn: 1000 }), &NOW),
            Plan::Quick,
            "exactly at the first USN is still readable"
        );
        assert_eq!(plan(Some(StoredUsn { journal_id: 7, next_usn: 999 }), &NOW), Plan::Full(FullReason::Wrapped));
        assert_eq!(
            plan(Some(StoredUsn { journal_id: 8, next_usn: 3000 }), &NOW),
            Plan::Full(FullReason::JournalChanged)
        );
        assert_eq!(plan(None, &NOW), Plan::Full(FullReason::NoPriorState));
        assert_eq!(
            plan(Some(StoredUsn { journal_id: 7, next_usn: 3000 }), &JournalState { active: false, ..NOW }),
            Plan::Full(FullReason::Inactive)
        );
    }

    #[test]
    fn more_than_a_fifth_of_records_changed_means_full() {
        assert_eq!(check_volume_of_changes(200, 1000), Plan::Quick);
        assert!(matches!(
            check_volume_of_changes(201, 1000),
            Plan::Full(FullReason::TooManyChanges { changed: 201, total: 1000 })
        ));
        assert_eq!(check_volume_of_changes(5, 0), Plan::Quick);
        assert!(FullReason::Wrapped.message().contains("USN journal wrapped"));
    }

    fn v2(frn: u64, usn: i64, reason: u32) -> Vec<u8> {
        let mut r = vec![0u8; 0x40];
        r[0..4].copy_from_slice(&0x40u32.to_le_bytes());
        r[4..6].copy_from_slice(&2u16.to_le_bytes());
        r[8..16].copy_from_slice(&(frn | (3u64 << 48)).to_le_bytes());
        r[24..32].copy_from_slice(&usn.to_le_bytes());
        r[40..44].copy_from_slice(&reason.to_le_bytes());
        r
    }

    #[test]
    fn parses_a_journal_buffer() {
        let mut b = 9000i64.to_le_bytes().to_vec();
        b.extend(v2(40, 100, 0x100));
        b.extend(v2(41, 140, 0x200));
        b.extend(v2(40, 180, 0x2));
        let (next, ch) = parse_read_buffer(&b).unwrap();
        assert_eq!(next, 9000);
        assert_eq!(ch.iter().map(|c| c.frn).collect::<Vec<_>>(), [40, 41, 40]);
        assert_eq!(ch[1], UsnChange { frn: 41, usn: 140, reason: 0x200 });
        assert_eq!(distinct_frns(&ch), vec![40, 41]);
    }

    #[test]
    fn damaged_buffers_are_errors() {
        assert!(parse_read_buffer(&[1, 2, 3]).is_err());
        let mut b = 1i64.to_le_bytes().to_vec();
        b.extend_from_slice(&0x1000u32.to_le_bytes()); // claims 4 KiB
        b.extend_from_slice(&[0; 0x30]);
        assert!(parse_read_buffer(&b).is_err());
        let mut b = 1i64.to_le_bytes().to_vec();
        b.extend_from_slice(&5u32.to_le_bytes()); // not a multiple of 8
        b.extend_from_slice(&[0; 0x40]);
        assert!(parse_read_buffer(&b).is_err());
        assert_eq!(parse_read_buffer(&1i64.to_le_bytes()).unwrap().1.len(), 0);
    }
}

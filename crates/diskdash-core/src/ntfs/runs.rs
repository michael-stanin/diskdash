//! NTFS data runs: `(length, offset)` pairs packed into variable-size fields.

use super::record::ParseError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Run {
    /// Logical cluster number, or `None` for a sparse (unallocated) run.
    pub lcn: Option<u64>,
    pub clusters: u64,
}

/// Decode a run list. Stops at the terminating zero byte or the end of `b`.
pub fn decode(b: &[u8]) -> Result<Vec<Run>, ParseError> {
    let mut runs = Vec::new();
    let (mut i, mut lcn) = (0usize, 0i64);
    while let Some(&head) = b.get(i) {
        if head == 0 {
            break;
        }
        let (len_size, off_size) = (usize::from(head & 0x0F), usize::from(head >> 4));
        if len_size == 0 || len_size > 8 || off_size > 8 {
            return Err(ParseError::BadRuns);
        }
        i += 1;
        let len_bytes = b.get(i..i + len_size).ok_or(ParseError::BadRuns)?;
        let clusters = len_bytes.iter().rev().fold(0u64, |a, &x| (a << 8) | u64::from(x));
        i += len_size;
        if off_size == 0 {
            runs.push(Run { lcn: None, clusters });
            continue;
        }
        let off_bytes = b.get(i..i + off_size).ok_or(ParseError::BadRuns)?;
        i += off_size;
        // Signed, little endian.
        let mut delta = off_bytes.iter().rev().fold(0i64, |a, &x| (a << 8) | i64::from(x));
        if off_bytes[off_size - 1] & 0x80 != 0 && off_size < 8 {
            delta |= -1i64 << (8 * off_size);
        }
        lcn = lcn.checked_add(delta).ok_or(ParseError::BadRuns)?;
        if lcn < 0 {
            return Err(ParseError::BadRuns);
        }
        runs.push(Run { lcn: Some(lcn as u64), clusters });
    }
    Ok(runs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_runs_with_negative_deltas_and_sparse_holes() {
        // 0x21: len 1 byte, off 2 bytes. 0x11: len 1, off 1 (signed).
        let b = [0x21, 0x18, 0x34, 0x12, 0x01, 0x10, 0x11, 0x05, 0xFC, 0x00];
        let r = decode(&b).unwrap();
        assert_eq!(r[0], Run { lcn: Some(0x1234), clusters: 0x18 });
        assert_eq!(r[1], Run { lcn: None, clusters: 0x10 });
        assert_eq!(r[2], Run { lcn: Some(0x1234 - 4), clusters: 5 });
    }

    #[test]
    fn rejects_truncated_and_oversized_fields() {
        assert!(decode(&[0x21, 0x18, 0x34]).is_err());
        assert!(decode(&[0x0F, 0x01]).is_err(), "zero length size");
        assert!(decode(&[0x19, 0x01]).is_err(), "offset size above 8");
        assert!(decode(&[0x11, 0x01, 0x80, 0x00]).is_err(), "negative absolute LCN");
        assert_eq!(decode(&[]).unwrap(), vec![]);
    }
}

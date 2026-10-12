//! Allocated size ("size on disk").

pub const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x10;
pub const FILE_ATTRIBUTE_SPARSE_FILE: u32 = 0x200;
pub const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
pub const FILE_ATTRIBUTE_COMPRESSED: u32 = 0x800;
pub const FILE_ATTRIBUTE_OFFLINE: u32 = 0x1000;
pub const FILE_ATTRIBUTE_RECALL_ON_OPEN: u32 = 0x4_0000;
pub const FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS: u32 = 0x40_0000;

/// Online-only cloud file: never opened for data (that would download it).
pub fn is_cloud_placeholder(attrs: u32) -> bool {
    attrs & (FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS | FILE_ATTRIBUTE_RECALL_ON_OPEN | FILE_ATTRIBUTE_OFFLINE) != 0
}

/// Opening even a metadata handle would trigger a recall for these.
pub fn must_not_open(attrs: u32) -> bool {
    attrs & (FILE_ATTRIBUTE_RECALL_ON_OPEN | FILE_ATTRIBUTE_OFFLINE) != 0
}

/// Whether the real allocation must be asked from the filesystem.
pub fn needs_query(attrs: u32, reparse_tag: u32) -> bool {
    attrs & (FILE_ATTRIBUTE_COMPRESSED | FILE_ATTRIBUTE_SPARSE_FILE) != 0
        || crate::reparse::needs_compressed_size(reparse_tag)
}

pub fn round_up(size: u64, cluster: u64) -> u64 {
    if cluster == 0 {
        return size;
    }
    size.div_ceil(cluster) * cluster
}

/// Allocated size from what is known:
/// - cloud placeholders: 0;
/// - `queried` (from `GetCompressedFileSizeW`/handle info) when available;
/// - otherwise logical size rounded up to whole clusters.
pub fn allocated(logical: u64, attrs: u32, cluster: u64, queried: Option<u64>) -> u64 {
    if is_cloud_placeholder(attrs) {
        return 0;
    }
    match queried {
        Some(q) => round_up(q, cluster),
        None => round_up(logical, cluster),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cases() {
        assert_eq!(allocated(1, 0, 4096, None), 4096, "cluster slack");
        assert_eq!(allocated(0, 0, 4096, None), 0);
        assert_eq!(allocated(8192, 0, 4096, None), 8192);
        assert_eq!(allocated(10 << 20, FILE_ATTRIBUTE_COMPRESSED, 4096, Some(70_000)), 73_728);
        assert_eq!(allocated(1 << 30, FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS, 4096, None), 0, "cloud placeholder");
        assert_eq!(allocated(1 << 30, FILE_ATTRIBUTE_OFFLINE, 4096, None), 0);
        assert!(needs_query(FILE_ATTRIBUTE_SPARSE_FILE, 0));
        assert!(needs_query(0, crate::reparse::IO_REPARSE_TAG_WOF));
        assert!(!needs_query(0, 0));
        assert!(must_not_open(FILE_ATTRIBUTE_RECALL_ON_OPEN));
        assert!(!must_not_open(FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS));
    }
}

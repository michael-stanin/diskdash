//! What to do with reparse points: traverse, record as a link, or record as a
//! file. Junctions and symlinks are never followed (no double counting).

pub const IO_REPARSE_TAG_MOUNT_POINT: u32 = 0xA000_0003;
pub const IO_REPARSE_TAG_SYMLINK: u32 = 0xA000_000C;
pub const IO_REPARSE_TAG_DEDUP: u32 = 0x8000_0013;
pub const IO_REPARSE_TAG_WOF: u32 = 0x8000_0017;
pub const IO_REPARSE_TAG_WCI: u32 = 0x8000_0018;
pub const IO_REPARSE_TAG_WCI_1: u32 = 0x9000_1018;
pub const IO_REPARSE_TAG_CLOUD: u32 = 0x9000_001A;
pub const IO_REPARSE_TAG_APPEXECLINK: u32 = 0x8000_001B;
pub const IO_REPARSE_TAG_PROJFS: u32 = 0x9000_001C;
pub const IO_REPARSE_TAG_LX_SYMLINK: u32 = 0xA000_001D;
pub const IO_REPARSE_TAG_ONEDRIVE: u32 = 0x8000_0021;
pub const IO_REPARSE_TAG_AF_UNIX: u32 = 0x8000_0023;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Enumerate children / count as a normal file.
    Normal,
    /// Record as a zero-size link entry; never traverse.
    Link,
}

/// Cloud-files tags: `IO_REPARSE_TAG_CLOUD` and `CLOUD_1`…`CLOUD_F`.
pub fn is_cloud(tag: u32) -> bool {
    tag & 0xFFFF_0FFF == IO_REPARSE_TAG_CLOUD || tag == IO_REPARSE_TAG_ONEDRIVE
}

/// Tags whose allocated size must come from `GetCompressedFileSizeW`.
pub fn needs_compressed_size(tag: u32) -> bool {
    matches!(tag, IO_REPARSE_TAG_DEDUP | IO_REPARSE_TAG_WOF)
}

pub fn known(tag: u32) -> bool {
    is_cloud(tag)
        || matches!(
            tag,
            IO_REPARSE_TAG_MOUNT_POINT
                | IO_REPARSE_TAG_SYMLINK
                | IO_REPARSE_TAG_DEDUP
                | IO_REPARSE_TAG_WOF
                | IO_REPARSE_TAG_WCI
                | IO_REPARSE_TAG_WCI_1
                | IO_REPARSE_TAG_APPEXECLINK
                | IO_REPARSE_TAG_PROJFS
                | IO_REPARSE_TAG_LX_SYMLINK
                | IO_REPARSE_TAG_AF_UNIX
        )
}

/// Decide how to treat an entry carrying `tag`.
pub fn action(tag: u32, is_dir: bool) -> Action {
    match tag {
        IO_REPARSE_TAG_MOUNT_POINT | IO_REPARSE_TAG_SYMLINK => Action::Link,
        IO_REPARSE_TAG_APPEXECLINK | IO_REPARSE_TAG_LX_SYMLINK | IO_REPARSE_TAG_AF_UNIX => Action::Link,
        IO_REPARSE_TAG_WCI | IO_REPARSE_TAG_WCI_1 => Action::Link,
        t if is_cloud(t) => Action::Normal,
        IO_REPARSE_TAG_DEDUP | IO_REPARSE_TAG_WOF | IO_REPARSE_TAG_PROJFS => Action::Normal,
        // Unknown: directories are not entered (could point anywhere); files are counted.
        _ if is_dir => Action::Link,
        _ => Action::Normal,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table() {
        let cases = [
            (IO_REPARSE_TAG_MOUNT_POINT, true, Action::Link),
            (IO_REPARSE_TAG_SYMLINK, true, Action::Link),
            (IO_REPARSE_TAG_SYMLINK, false, Action::Link),
            (IO_REPARSE_TAG_CLOUD, true, Action::Normal),
            (0x9000_701A, true, Action::Normal), // CLOUD_7
            (0x9000_701A, false, Action::Normal),
            (IO_REPARSE_TAG_DEDUP, false, Action::Normal),
            (IO_REPARSE_TAG_WOF, false, Action::Normal),
            (IO_REPARSE_TAG_APPEXECLINK, false, Action::Link),
            (IO_REPARSE_TAG_WCI, true, Action::Link),
            (0x1234_5678, true, Action::Link),
            (0x1234_5678, false, Action::Normal),
        ];
        for (tag, dir, expected) in cases {
            assert_eq!(action(tag, dir), expected, "tag {tag:#x} dir {dir}");
        }
    }

    #[test]
    fn cloud_variants() {
        for n in 0..=0xFu32 {
            assert!(is_cloud(0x9000_001A | (n << 12)));
        }
        assert!(!is_cloud(IO_REPARSE_TAG_SYMLINK));
        assert!(!known(0x1234_5678));
        assert!(known(IO_REPARSE_TAG_WOF));
    }
}

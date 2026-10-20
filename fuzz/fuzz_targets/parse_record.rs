#![no_main]
//! Any bytes may be handed to the MFT record parser; it must return an error, never panic.
use diskdash_core::ntfs::record::parse_record;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let mut buf = data.to_vec();
    let _ = parse_record(&mut buf);
    // Also try with a valid signature and update-sequence header so the attribute walk is reached.
    if buf.len() >= 0x30 {
        let mut b = data.to_vec();
        b[0..4].copy_from_slice(b"FILE");
        b[4..6].copy_from_slice(&0x30u16.to_le_bytes());
        b[6..8].copy_from_slice(&1u16.to_le_bytes());
        let _ = parse_record(&mut b);
    }
});

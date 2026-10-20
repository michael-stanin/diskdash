#![no_main]
use diskdash_core::ntfs::runs::decode;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = decode(data);
});

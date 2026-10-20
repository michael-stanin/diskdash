# cargo-fuzz targets for the MFT parser.
#   cargo +nightly fuzz run parse_record
#   cargo +nightly fuzz run decode_runs
# Not built by the normal workspace. The same inputs are also covered by the
# truncation and bit-flip unit test in crates/diskdash-core/src/ntfs/record.rs.

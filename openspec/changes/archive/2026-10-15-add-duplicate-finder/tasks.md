## 1. Candidate selection (crates/diskdash-analysis::duplicates)

- [x] 1.1 Catalog migration: `hash_cache`, `dup_runs`, `dup_groups`, `dup_members`, `dup_suggestions`
- [x] 1.2 Size grouping across attached snapshot DBs with min-size, zero-byte, reparse, cloud-placeholder and protection/exclusion filters, and `hardlink_of`/file-id collapse; `cargo test` on a test-fixtures tree with hard links and placeholder attributes

## 2. Hashing engine

- [x] 2.1 Read-only opener via the `windows` crate (GENERIC_READ, full sharing, SEQUENTIAL_SCAN), attribute re-check, size/mtime verification, and handle file-id capture with same-id collapse; tests for locked, changed, access-denied and hard-linked fixture files
- [x] 2.2 Partial and full BLAKE3 (`xxhash-rust` xxh3 behind a feature/config) with cache lookup/store; tests for the same-ends/different-middle case and cache hits
- [x] 2.3 Scheduler on job-runner worker threads: per-physical-disk queues, one reader per HDD, bounded pool per SSD, cancel and SSE progress, panic isolation; test with a fake disk map asserting HDD concurrency is 1

## 3. Results

- [x] 3.1 Group builder and wasted-space computation; tests (3-copy group, hard-link exclusion)
- [x] 3.2 Keep rules with deterministic tie-break → `dup_suggestions` (consumable by cleanup-plans); tests for each rule; test that fixture files' mtime/hash are unchanged after a run
- [x] 3.3 Optional near-duplicate folders (inverted index, Jaccard, pair cap); test with a copied fixture folder

## 4. API and UI

- [x] 4.1 Routes: start/cancel run, list groups (sort/filter, 20 per page), apply keep rule, near-dup folders; ts-rs types; integration tests
- [x] 4.2 Duplicates tab and cross-drive page: group expansion, keep-rule picker, atime notice, skipped-files summary, progress with cancel. States: group skeletons; "never run" and "no duplicates found" empty states; error with retry. Component tests

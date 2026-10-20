//! The request loop. Transport-agnostic (any `Read + Write`), so it is tested
//! in-process with a fake [`Backend`]; the real pipe and Win32 backend live in
//! [`crate::win`].
//!
//! Safety properties enforced here, not in the backend:
//! - nothing is served before a correct secret (compared in constant time);
//! - only [`Request`] variants exist, so there is no way to ask for a path, a
//!   write, a process or a handle;
//! - volume ids and disk numbers must come from the backend's own list.

use std::io::{Read, Write};

use diskdash_core::ntfs::model::{MftEntry, MftInfo, ScanTally};
use diskdash_core::ntfs::usn::{JournalState, StoredUsn};
use subtle::ConstantTimeEq;

use crate::proto::{
    decode, read_frame_bytes, write_frame, ErrorCode, FrameError, HelperVolume, PrefetchRun, Request, Response,
    SmartCounters, UsnOutcome, BATCH_ENTRIES, PROTOCOL_VERSION,
};

/// What the helper can do. Every method is read-only by construction.
pub trait Backend {
    /// Fixed internal volumes, from the helper's own enumeration.
    fn volumes(&self) -> Vec<HelperVolume>;
    fn mft_info(&self, volume_id: &str) -> Result<MftInfo, String>;
    /// Stream entries in batches; `sink` returns false to stop (client gone).
    fn mft_scan(
        &self,
        volume_id: &str,
        sink: &mut dyn FnMut(Vec<MftEntry>) -> bool,
    ) -> Result<(ScanTally, Option<JournalState>), String>;
    fn usn_read(&self, volume_id: &str, stored: Option<StoredUsn>, total_records: u64) -> Result<UsnOutcome, String>;
    fn smart(&self, disk_number: u32) -> Result<SmartCounters, String>;
    fn prefetch(&self) -> Result<Vec<PrefetchRun>, String>;
}

fn error(code: ErrorCode, message: impl Into<String>) -> Response {
    Response::Error { code, message: message.into() }
}

/// Serve one client until it disconnects. Returns `Ok(())` for a normal end or a rejected client.
pub fn serve<S: Read + Write>(
    stream: &mut S,
    secret: &[u8],
    backend: &dyn Backend,
    on_activity: &dyn Fn(),
) -> Result<(), FrameError> {
    // 1. Authenticate.
    let first = read_frame_bytes(stream)?;
    match decode::<Request>(&first) {
        Ok(Request::Hello { secret: given, version }) => {
            let ok: bool = given.len() == secret.len() && bool::from(given.ct_eq(secret));
            if !ok {
                tracing::warn!("rejected a client with a wrong secret");
                write_frame(stream, &error(ErrorCode::NotAuthenticated, "authentication failed"))?;
                return Ok(());
            }
            if version != PROTOCOL_VERSION {
                write_frame(
                    stream,
                    &error(
                        ErrorCode::BadVersion,
                        format!("protocol {version} is not supported (expected {PROTOCOL_VERSION})"),
                    ),
                )?;
                return Ok(());
            }
            write_frame(stream, &Response::Welcome { version: PROTOCOL_VERSION })?;
        }
        _ => {
            tracing::warn!("rejected a client that did not authenticate first");
            write_frame(stream, &error(ErrorCode::NotAuthenticated, "authenticate first"))?;
            return Ok(());
        }
    }

    // 2. Requests.
    let mut volumes = backend.volumes();
    loop {
        let bytes = match read_frame_bytes(stream) {
            Ok(b) => b,
            Err(FrameError::Closed) => return Ok(()),
            Err(e) => return Err(e),
        };
        on_activity();
        let req = match decode::<Request>(&bytes) {
            Ok(Request::Hello { .. }) | Err(_) => {
                tracing::warn!("rejected an unknown operation");
                write_frame(stream, &error(ErrorCode::UnknownOperation, "that operation is not supported"))?;
                continue;
            }
            Ok(r) => r,
        };
        let known = |volumes: &[HelperVolume], id: &str| -> Result<HelperVolume, Response> {
            let v = volumes
                .iter()
                .find(|v| v.id == id)
                .cloned()
                .ok_or_else(|| error(ErrorCode::UnknownVolume, "not a fixed internal volume"))?;
            if !v.filesystem.eq_ignore_ascii_case("NTFS") {
                return Err(error(ErrorCode::NotNtfs, "MFT access supports NTFS only"));
            }
            Ok(v)
        };
        match req {
            Request::Hello { .. } => unreachable!("handled above"),
            Request::Volumes => {
                volumes = backend.volumes();
                write_frame(stream, &Response::Volumes(volumes.clone()))?;
            }
            Request::MftInfo { volume_id } => {
                let resp = match known(&volumes, &volume_id) {
                    Err(e) => e,
                    Ok(_) => backend
                        .mft_info(&volume_id)
                        .map(Response::MftInfo)
                        .unwrap_or_else(|m| error(ErrorCode::Failed, m)),
                };
                write_frame(stream, &resp)?;
            }
            Request::MftScan { volume_id } => {
                if let Err(e) = known(&volumes, &volume_id) {
                    write_frame(stream, &e)?;
                    continue;
                }
                let mut write_err: Option<FrameError> = None;
                let result = backend.mft_scan(&volume_id, &mut |batch| {
                    for chunk in batch.chunks(BATCH_ENTRIES) {
                        if let Err(e) = write_frame(stream, &Response::MftBatch(chunk.to_vec())) {
                            write_err = Some(e);
                            return false;
                        }
                    }
                    on_activity();
                    true
                });
                if let Some(e) = write_err {
                    return Err(e);
                }
                let resp = match result {
                    Ok((tally, journal)) => Response::MftDone { tally, journal },
                    Err(m) => error(ErrorCode::Failed, m),
                };
                write_frame(stream, &resp)?;
            }
            Request::UsnRead { volume_id, stored, total_records } => {
                let resp = match known(&volumes, &volume_id) {
                    Err(e) => e,
                    Ok(_) => backend
                        .usn_read(&volume_id, stored, total_records)
                        .map(Response::Usn)
                        .unwrap_or_else(|m| error(ErrorCode::Failed, m)),
                };
                write_frame(stream, &resp)?;
            }
            Request::SmartCounters { disk_number } => {
                let resp = if volumes.iter().any(|v| v.disk_numbers.contains(&disk_number)) {
                    backend.smart(disk_number).map(Response::Smart).unwrap_or_else(|m| error(ErrorCode::Failed, m))
                } else {
                    error(ErrorCode::UnknownVolume, "not one of the internal disks")
                };
                write_frame(stream, &resp)?;
            }
            Request::PrefetchLastRun => {
                let resp = backend.prefetch().map(Response::Prefetch).unwrap_or_else(|m| error(ErrorCode::Failed, m));
                write_frame(stream, &resp)?;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proto::read_frame;
    use std::cell::Cell;
    use std::io::Cursor;

    struct Pair {
        input: Cursor<Vec<u8>>,
        output: Vec<u8>,
    }

    impl Read for Pair {
        fn read(&mut self, b: &mut [u8]) -> std::io::Result<usize> {
            self.input.read(b)
        }
    }

    impl Write for Pair {
        fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
            self.output.write(b)
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    struct Fake {
        scans: Cell<u32>,
    }

    fn entry(frn: u64) -> MftEntry {
        MftEntry {
            frn,
            seq: 1,
            is_dir: false,
            attrs: 0x20,
            ctime: 0,
            mtime: 0,
            atime: 0,
            names: vec![],
            size: 1,
            allocated: 0,
            resident: true,
            compressed_or_sparse: false,
            reparse_tag: 0,
        }
    }

    impl Backend for Fake {
        fn volumes(&self) -> Vec<HelperVolume> {
            vec![
                HelperVolume {
                    id: "ntfs-vol".into(),
                    letters: vec!["C".into()],
                    filesystem: "NTFS".into(),
                    disk_numbers: vec![0],
                },
                HelperVolume {
                    id: "exfat-vol".into(),
                    letters: vec!["E".into()],
                    filesystem: "exFAT".into(),
                    disk_numbers: vec![2],
                },
            ]
        }

        fn mft_info(&self, _: &str) -> Result<MftInfo, String> {
            Ok(MftInfo { bytes_per_cluster: 4096, record_size: 1024, record_count: 10, serial: 1 })
        }

        fn mft_scan(
            &self,
            _: &str,
            sink: &mut dyn FnMut(Vec<MftEntry>) -> bool,
        ) -> Result<(ScanTally, Option<JournalState>), String> {
            self.scans.set(self.scans.get() + 1);
            sink((0..10_000).map(entry).collect());
            Ok((ScanTally { records: 10_000, bad_records: 0 }, None))
        }

        fn usn_read(&self, _: &str, _: Option<StoredUsn>, _: u64) -> Result<UsnOutcome, String> {
            Ok(UsnOutcome::Full { reason: "no earlier fast scan".into(), state: None })
        }

        fn smart(&self, d: u32) -> Result<SmartCounters, String> {
            Ok(SmartCounters { predict_failure: Some(false), temperature_c: Some(30 + d as i32) })
        }

        fn prefetch(&self) -> Result<Vec<PrefetchRun>, String> {
            Ok(vec![PrefetchRun { exe: "CHROME.EXE".into(), last_run: 5 }])
        }
    }

    const SECRET: [u8; 32] = [7; 32];

    fn run(frames: Vec<Request>) -> Vec<Response> {
        let mut input = Vec::new();
        for f in &frames {
            write_frame(&mut input, f).unwrap();
        }
        let mut pair = Pair { input: Cursor::new(input), output: Vec::new() };
        serve(&mut pair, &SECRET, &Fake { scans: Cell::new(0) }, &|| {}).unwrap();
        let mut out = Cursor::new(pair.output);
        let mut v = Vec::new();
        while let Ok(r) = read_frame::<Response>(&mut out) {
            v.push(r);
        }
        v
    }

    fn hello(secret: &[u8]) -> Request {
        Request::Hello { secret: secret.to_vec(), version: PROTOCOL_VERSION }
    }

    #[test]
    fn a_client_without_the_secret_gets_nothing() {
        let r = run(vec![hello(&[1; 32]), Request::Volumes]);
        assert_eq!(r.len(), 1, "closed after the rejection: {r:?}");
        assert!(matches!(r[0], Response::Error { code: ErrorCode::NotAuthenticated, .. }));
        let r = run(vec![hello(&[7; 31]), Request::Volumes]);
        assert!(matches!(r[0], Response::Error { code: ErrorCode::NotAuthenticated, .. }), "wrong length");
    }

    #[test]
    fn a_request_before_hello_is_rejected() {
        let r = run(vec![Request::Volumes]);
        assert!(matches!(r[0], Response::Error { code: ErrorCode::NotAuthenticated, .. }));
        assert_eq!(r.len(), 1);
    }

    #[test]
    fn a_wrong_protocol_version_is_refused() {
        let r = run(vec![Request::Hello { secret: SECRET.to_vec(), version: 99 }]);
        assert!(matches!(r[0], Response::Error { code: ErrorCode::BadVersion, .. }));
    }

    #[test]
    fn authenticated_clients_can_list_volumes_and_stream_a_scan_in_batches() {
        let r = run(vec![hello(&SECRET), Request::Volumes, Request::MftScan { volume_id: "ntfs-vol".into() }]);
        assert!(matches!(r[0], Response::Welcome { version: PROTOCOL_VERSION }));
        assert!(matches!(&r[1], Response::Volumes(v) if v.len() == 2));
        let batches: Vec<usize> =
            r.iter().filter_map(|x| if let Response::MftBatch(b) = x { Some(b.len()) } else { None }).collect();
        assert_eq!(batches, [4096, 4096, 1808], "10 000 entries in frames of at most {BATCH_ENTRIES}");
        assert!(matches!(r.last(), Some(Response::MftDone { tally, .. }) if tally.records == 10_000));
    }

    #[test]
    fn only_the_helpers_own_ntfs_volumes_are_accepted() {
        let r = run(vec![
            hello(&SECRET),
            Request::MftScan { volume_id: r"C:\Windows".into() },
            Request::MftScan { volume_id: "exfat-vol".into() },
            Request::MftInfo { volume_id: "ntfs-vol".into() },
            Request::UsnRead { volume_id: "../../etc".into(), stored: None, total_records: 0 },
        ]);
        assert!(matches!(r[1], Response::Error { code: ErrorCode::UnknownVolume, .. }), "a path is not a volume id");
        assert!(matches!(r[2], Response::Error { code: ErrorCode::NotNtfs, .. }));
        assert!(matches!(r[3], Response::MftInfo(_)));
        assert!(matches!(r[4], Response::Error { code: ErrorCode::UnknownVolume, .. }));
    }

    #[test]
    fn disk_numbers_must_be_internal_disks() {
        let r = run(vec![
            hello(&SECRET),
            Request::SmartCounters { disk_number: 0 },
            Request::SmartCounters { disk_number: 9 },
            Request::PrefetchLastRun,
        ]);
        assert!(matches!(&r[1], Response::Smart(s) if s.temperature_c == Some(30)));
        assert!(matches!(r[2], Response::Error { code: ErrorCode::UnknownVolume, .. }));
        assert!(matches!(&r[3], Response::Prefetch(p) if p[0].exe == "CHROME.EXE"));
    }

    #[test]
    fn an_unknown_operation_is_rejected_and_the_session_continues() {
        let mut input = Vec::new();
        write_frame(&mut input, &hello(&SECRET)).unwrap();
        // A frame whose variant index does not exist.
        input.extend_from_slice(&2u32.to_le_bytes());
        input.extend_from_slice(&[0x7F, 0x00]);
        write_frame(&mut input, &Request::Volumes).unwrap();
        // A second Hello is not an operation either.
        write_frame(&mut input, &hello(&SECRET)).unwrap();
        let mut pair = Pair { input: Cursor::new(input), output: Vec::new() };
        serve(&mut pair, &SECRET, &Fake { scans: Cell::new(0) }, &|| {}).unwrap();
        let mut out = Cursor::new(pair.output);
        let r: Vec<Response> = std::iter::from_fn(|| read_frame::<Response>(&mut out).ok()).collect();
        assert!(matches!(r[1], Response::Error { code: ErrorCode::UnknownOperation, .. }));
        assert!(matches!(r[2], Response::Volumes(_)), "still served");
        assert!(matches!(r[3], Response::Error { code: ErrorCode::UnknownOperation, .. }));
    }

    #[test]
    fn the_request_enum_has_no_path_or_write_operation() {
        // Compile-time documentation as a test: every variant is listed, and none takes a path.
        let all = [
            Request::Volumes,
            Request::PrefetchLastRun,
            Request::MftInfo { volume_id: String::new() },
            Request::MftScan { volume_id: String::new() },
            Request::UsnRead { volume_id: String::new(), stored: None, total_records: 0 },
            Request::SmartCounters { disk_number: 0 },
            Request::Hello { secret: vec![], version: 0 },
        ];
        for r in all {
            match r {
                Request::Hello { .. }
                | Request::Volumes
                | Request::MftInfo { .. }
                | Request::MftScan { .. }
                | Request::UsnRead { .. }
                | Request::SmartCounters { .. }
                | Request::PrefetchLastRun => {}
            }
        }
    }
}

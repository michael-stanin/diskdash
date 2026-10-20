//! `diskdash-helper.exe --pipe diskdash-<32 hex> --secret <64 hex> [--parent-pid N]`
//!
//! Started by diskdash through a UAC prompt. Creates one user-restricted pipe,
//! serves the one client that connects, and exits when it disconnects or after
//! ten idle minutes. It never writes anything.

#[cfg(windows)]
fn main() -> std::process::ExitCode {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use diskdash_helper::win::{accept_pipe, current_user_sid, SystemBackend};
    use diskdash_helper::{pipe_path, serve, valid_pipe_name};

    const IDLE_EXIT: Duration = Duration::from_secs(10 * 60);

    tracing_subscriber::fmt().with_writer(std::io::stderr).init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let get = |flag: &str| args.windows(2).find(|w| w[0] == flag).map(|w| w[1].clone());
    let (Some(pipe), Some(secret_hex)) = (get("--pipe"), get("--secret")) else {
        eprintln!("usage: diskdash-helper --pipe diskdash-<32 hex> --secret <64 hex>");
        return std::process::ExitCode::from(2);
    };
    if !valid_pipe_name(&pipe) {
        eprintln!("invalid pipe name");
        return std::process::ExitCode::from(2);
    }
    let Some(secret) = decode_hex(&secret_hex).filter(|s| s.len() == 32) else {
        eprintln!("the secret must be 64 hex characters");
        return std::process::ExitCode::from(2);
    };

    let sid = match current_user_sid() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("cannot determine the current user: {e}");
            return std::process::ExitCode::from(1);
        }
    };
    let backend = SystemBackend::new();
    let now = || SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
    static LAST: AtomicU64 = AtomicU64::new(0);
    LAST.store(now(), Ordering::Relaxed);
    // Idle watchdog: a client that connects and goes silent must not keep an elevated process alive.
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_secs(15));
        if now().saturating_sub(LAST.load(Ordering::Relaxed)) > IDLE_EXIT.as_secs() {
            tracing::info!("idle for ten minutes; exiting");
            std::process::exit(0);
        }
    });

    let mut stream = match accept_pipe(&pipe_path(&pipe), &sid) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("{e}");
            return std::process::ExitCode::from(1);
        }
    };
    LAST.store(now(), Ordering::Relaxed);
    let result = serve(&mut stream, &secret, &backend, &|| LAST.store(now(), Ordering::Relaxed));
    if let Err(e) = result {
        tracing::warn!(error = %e, "session ended with an error");
    }
    std::process::ExitCode::SUCCESS
}

#[cfg(windows)]
fn decode_hex(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) || !s.is_ascii() {
        return None;
    }
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok()).collect()
}

#[cfg(not(windows))]
fn main() {
    eprintln!("diskdash-helper runs on Windows only");
}

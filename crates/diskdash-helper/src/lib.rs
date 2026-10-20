//! `diskdash-helper`: the strictly read-only elevated half of diskdash.
//!
//! The web server never runs elevated. When the user clicks "Enable elevated
//! features" it starts this program through a UAC prompt. The two talk over a
//! named pipe that only the same user can open, authenticated with a per-launch
//! secret, using the closed set of read-only operations in [`proto::Request`].
//!
//! The library holds the protocol and the transport-independent request loop so
//! the server uses the same types; the Windows pieces are in [`win`].

pub mod client;
#[cfg(windows)]
pub mod pipe_client;
pub mod proto;
pub mod service;
#[cfg(windows)]
pub mod win;

pub use proto::{ErrorCode, HelperVolume, PrefetchRun, Request, Response, SmartCounters, UsnOutcome, PROTOCOL_VERSION};
pub use service::{serve, Backend};

/// Where the helper's pipe lives for a given launch token.
pub fn pipe_path(name: &str) -> String {
    format!(r"\\.\pipe\{name}")
}

/// A pipe name is `diskdash-` plus 32 hex characters; anything else is refused so the
/// command line cannot be used to point the helper at another pipe.
pub fn valid_pipe_name(name: &str) -> bool {
    name.strip_prefix("diskdash-").is_some_and(|rest| rest.len() == 32 && rest.bytes().all(|b| b.is_ascii_hexdigit()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pipe_names_are_validated() {
        assert!(valid_pipe_name("diskdash-0123456789abcdef0123456789abcdef"));
        assert!(!valid_pipe_name("diskdash-short"));
        assert!(!valid_pipe_name(r"..\evil"));
        assert!(!valid_pipe_name("other-0123456789abcdef0123456789abcdef"));
        assert_eq!(pipe_path("diskdash-x"), r"\\.\pipe\diskdash-x");
    }
}

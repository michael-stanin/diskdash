//! Command-line configuration. The server may only ever bind to 127.0.0.1.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};

pub const DEFAULT_PORT: u16 = 8765;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub bind: IpAddr,
    pub port: u16,
    /// Dev mode: enables `/api/v1/session` (token for the Vite dev server)
    /// and test-only job kinds.
    pub dev: bool,
    /// Open the default browser once the server is listening.
    pub open_browser: bool,
    /// Stop when the last browser page has been closed (see `idle`).
    pub exit_when_idle: bool,
    /// Seconds with no page open before `exit_when_idle` stops the server.
    pub idle_secs: u64,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ConfigError {
    #[error("only 127.0.0.1 is allowed as the bind address (got {0})")]
    NonLoopback(String),
    #[error("invalid port: {0}")]
    BadPort(String),
    #[error("--idle-secs must be between 1 and 3600 (got {0})")]
    BadIdle(String),
    #[error("unknown argument: {0}")]
    Unknown(String),
    #[error("missing value for {0}")]
    MissingValue(&'static str),
}

impl Default for Config {
    fn default() -> Self {
        Self {
            bind: IpAddr::V4(Ipv4Addr::LOCALHOST),
            port: DEFAULT_PORT,
            dev: false,
            open_browser: false,
            exit_when_idle: false,
            idle_secs: 30,
        }
    }
}

/// Bind the listener, turning "address in use" into an actionable message.
pub async fn bind(cfg: &Config) -> Result<tokio::net::TcpListener, String> {
    match tokio::net::TcpListener::bind(cfg.addr()).await {
        Ok(l) => Ok(l),
        Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => {
            Err(format!("port {} is already in use; start with -Port <n> (or --port <n>)", cfg.port))
        }
        Err(e) => Err(format!("cannot listen on {}: {e}", cfg.addr())),
    }
}

impl Config {
    pub fn addr(&self) -> SocketAddr {
        SocketAddr::new(self.bind, self.port)
    }

    /// Parse `--port <n>`, `--bind <ip>`, `--dev`, `--open`, `--exit-when-idle`, `--idle-secs <n>`.
    pub fn from_args<I: IntoIterator<Item = String>>(args: I) -> Result<Self, ConfigError> {
        let mut cfg = Config::default();
        let mut it = args.into_iter();
        while let Some(arg) = it.next() {
            match arg.as_str() {
                "--port" => {
                    let v = it.next().ok_or(ConfigError::MissingValue("--port"))?;
                    cfg.port = v.parse().map_err(|_| ConfigError::BadPort(v.clone()))?;
                    if cfg.port == 0 {
                        return Err(ConfigError::BadPort(v));
                    }
                }
                "--bind" => {
                    let v = it.next().ok_or(ConfigError::MissingValue("--bind"))?;
                    cfg.bind = parse_bind(&v)?;
                }
                "--dev" => cfg.dev = true,
                "--open" => cfg.open_browser = true,
                "--exit-when-idle" => cfg.exit_when_idle = true,
                "--idle-secs" => {
                    let v = it.next().ok_or(ConfigError::MissingValue("--idle-secs"))?;
                    cfg.idle_secs = v.parse().ok().filter(|n| (1..=3600).contains(n)).ok_or(ConfigError::BadIdle(v))?;
                }
                other => return Err(ConfigError::Unknown(other.to_string())),
            }
        }
        Ok(cfg)
    }
}

/// Accept only the IPv4 loopback address 127.0.0.1.
pub fn parse_bind(v: &str) -> Result<IpAddr, ConfigError> {
    match v.parse::<IpAddr>() {
        Ok(IpAddr::V4(ip)) if ip == Ipv4Addr::LOCALHOST => Ok(IpAddr::V4(ip)),
        _ => Err(ConfigError::NonLoopback(v.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(s: &str) -> Vec<String> {
        s.split_whitespace().map(String::from).collect()
    }

    #[test]
    fn defaults_to_loopback_8765() {
        let c = Config::from_args(Vec::new()).unwrap();
        assert_eq!(c.addr().to_string(), "127.0.0.1:8765");
    }

    #[test]
    fn rejects_any_non_loopback_bind() {
        for bad in ["0.0.0.0", "::", "::1", "192.168.1.10", "localhost"] {
            let err = Config::from_args(args(&format!("--bind {bad}"))).unwrap_err();
            assert_eq!(err, ConfigError::NonLoopback(bad.into()));
            assert!(err.to_string().contains("only 127.0.0.1 is allowed"));
        }
    }

    #[tokio::test]
    async fn busy_port_gives_clear_error() {
        let first = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = first.local_addr().unwrap().port();
        let cfg = Config { port, ..Config::default() };
        let err = bind(&cfg).await.unwrap_err();
        assert!(err.contains(&format!("port {port} is already in use")), "{err}");
    }

    #[test]
    fn parses_port_and_flags() {
        let c = Config::from_args(args("--port 9000 --dev --open")).unwrap();
        assert_eq!((c.port, c.dev, c.open_browser), (9000, true, true));
        assert!(Config::from_args(args("--port 0")).is_err());
        assert!(Config::from_args(args("--port x")).is_err());
        assert!(Config::from_args(args("--nope")).is_err());
        let c = Config::from_args(args("--exit-when-idle --idle-secs 5")).unwrap();
        assert_eq!((c.exit_when_idle, c.idle_secs), (true, 5));
        let d = Config::from_args(args("--open")).unwrap();
        assert_eq!((d.exit_when_idle, d.idle_secs), (false, 30), "off unless asked for");
        assert!(Config::from_args(args("--idle-secs 0")).is_err());
        assert!(Config::from_args(args("--idle-secs 99999")).is_err());
        assert!(Config::from_args(args("--idle-secs")).is_err());
    }
}

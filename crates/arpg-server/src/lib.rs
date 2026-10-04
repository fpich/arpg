pub mod bridge;
pub mod policy;
pub mod quic;
pub mod ratelimit;
pub mod session;

pub use session::{Session, SessionState, PROTOCOL_VERSION};

/// Listener socket binding for the game server. Always loopback: the
/// server accepts local clients only and must never be exposed publicly.
pub const SERVER_BIND_ADDR: &str = "127.0.0.1:0";

pub const PROTOCOL_DOC: &str = "docs/PROTOCOL.md";

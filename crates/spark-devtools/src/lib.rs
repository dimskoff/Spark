//! Spark devtools: the live link between a running game and Spark Editor.
//!
//! ```text
//!  spark-editor                               spark <game> --devtools 127.0.0.1:PORT
//!  ┌──────────────┐   EditorCommand (JSON)   ┌──────────────────────┐
//!  │ Server       │ ───────────────────────▶ │ Link (in the runner) │
//!  │ (listens)    │ ◀─────────────────────── │                      │
//!  └──────────────┘   GameMessage (JSON)     └──────────────────────┘
//! ```
//!
//! - Transport: TCP on localhost, one JSON object per line (easy to debug with `nc`).
//! - The editor listens, the game connects (the editor passes the address with `--devtools`).
//! - Logs are NOT sent here: the editor reads the game's stdout / stderr.
//! - Both sides do socket I/O on background threads: a slow or dead peer never stalls a frame.
//! - Versioned: [`PROTOCOL_VERSION`] travels in [`GameMessage::Hello`]; unknown messages are skipped.
#![forbid(unsafe_code)]

mod link;
mod protocol;

pub use link::{GameHello, GameLink, Link, MAX_BAD_ATTEMPTS, Server, ServerEvent, generate_token, tokens_match};
pub use protocol::*;

/// Bumped on incompatible changes of [`GameMessage`] / [`EditorCommand`] or of the handshake.
///
/// - 3: the editor authenticates the game: it hands out a token (`SPARK_DEVTOOLS_TOKEN`) and drops connections whose
///   `Hello` does not carry it. A v2 game already sends the token, so it still works; a v1 game (no token) is rejected.
pub const PROTOCOL_VERSION: u32 = 3;

/// Oldest game protocol the editor still talks to (messages did not change between 2 and 3).
pub const MIN_PROTOCOL_VERSION: u32 = 2;

/// Environment variable with the session token the editor hands to the game (sent back in `Hello`).
pub const TOKEN_ENV: &str = "SPARK_DEVTOOLS_TOKEN";

/// The runner flag that enables devtools: `spark game --devtools 127.0.0.1:50123`.
pub const FLAG: &str = "--devtools";

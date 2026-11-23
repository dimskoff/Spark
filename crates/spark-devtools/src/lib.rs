//! Spark devtools: the live link between a running game and Spark Editor.
//!
//! ```text
//!  spark-editor                               spark <game> --devtools 127.0.0.1:PORT
//!  в”Њв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”ђ   EditorCommand (JSON)   в”Њв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”ђ
//!  в”‚ Server       в”‚ в”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв–¶ в”‚ Link (in the runner) в”‚
//!  в”‚ (listens)    в”‚ в—Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђ в”‚                      в”‚
//!  в””в”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”   GameMessage (JSON)     в””в”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”
//! ```
//!
//! - Transport: TCP on localhost, one JSON object per line (easy to debug with `nc`).
//! - The editor listens, the game connects (the editor passes the address with `--devtools`).
//! - Logs are NOT sent here: the editor reads the game's stdout / stderr.
//! - Both sides do socket I/O on background threads: a slow or dead peer never stalls a frame.
//! - Versioned: [`PROTOCOL_VERSION`] travels in [`GameMessage::Hello`]; unknown messages are skipped.
#![forbid(unsafe_code)]

pub mod command_json;
mod link;
mod protocol;

pub use link::{GameHello, GameLink, Link, MAX_BAD_ATTEMPTS, Server, ServerEvent, generate_token, tokens_match};
pub use protocol::*;

/// Bumped on incompatible changes of [`GameMessage`] / [`EditorCommand`] or of the handshake.
///
/// - 3: the editor authenticates the game: it hands out a token (`SPARK_DEVTOOLS_TOKEN`) and drops connections whose
///   `Hello` does not carry it. A v2 game already sends the token, so it still works; a v1 game (no token) is rejected.
///
/// - 4: commands. [`EditorCommand::Command`] carries the JSON form of a `spark_core::Command` (see [`command_json`]),
///   answered by [`GameMessage::CommandResult`]; [`ObjectInfo`] gains `node_id`. `SetObject` stays and now expands to
///   a batch of `SetProperty` in the game. A v2/v3 game ignores the new message, so the editor must not rely on the
///   result when the game's `Hello` says it is older than 4.
pub const PROTOCOL_VERSION: u32 = 4;

/// Oldest game protocol the editor still talks to (messages only grew since: 3 changed the handshake, 4 added messages).
pub const MIN_PROTOCOL_VERSION: u32 = 2;

/// Environment variable with the session token the editor hands to the game (sent back in `Hello`).
pub const TOKEN_ENV: &str = "SPARK_DEVTOOLS_TOKEN";

/// The runner flag that enables devtools: `spark game --devtools 127.0.0.1:50123`.
pub const FLAG: &str = "--devtools";

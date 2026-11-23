//! Spark Engine scene files: `.scene` / `.prefab` (TOML, ADR 0003) <-> [`SceneFile`].
//!
//! - [`parse`]: text -> [`Loaded`] (the document and the `format_version` it declared). Older formats are migrated in
//!   memory ([`migrate`]); a file newer than [`FORMAT_VERSION`] is an error. Every error has the line of the key or
//!   value it is about, and unknown component / field names come with a did-you-mean.
//! - [`to_string`]: document -> canonical text, byte-for-byte deterministic. Parts that cannot be saved (a mesh made
//!   by `Mesh.custom`, texture handles) are an error naming the node; [`to_string_lossy`] saves the rest and reports
//!   what it left out.
//! - [`migrate_text`]: the text of a file rewritten in the current format (`spark migrate`).
//!
//! The document model, [`spark_core::doc::capture`] and [`spark_core::doc::instantiate`] live in `spark-core`, which has no
//! parser. The format is described in `docs/SCENE_FORMAT.md`.
//!
//! ```
//! use spark_core::{World, doc::{capture, instantiate}};
//!
//! let mut world = World::new();
//! let crate_ = world.spawn_empty();
//! world.scene[crate_].name = "Crate".into();
//! world.scene[crate_].transform.position.y = 0.5;
//!
//! let text = spark_scene::to_string(&capture(&mut world)).unwrap();
//! assert!(text.contains("name = \"Crate\"") && text.contains("position = [0, 0.5, 0]"));
//!
//! let mut other = World::new();
//! instantiate(&spark_scene::parse(&text).unwrap().file, &mut other).unwrap();
//! assert_eq!(other.scene[other.scene.find("Crate").unwrap()].transform.position.y, 0.5);
//! ```
#![forbid(unsafe_code)]

mod error;
pub mod migrate;
mod prefab;
mod read;
mod text;
mod tree;
mod world;
mod write;

pub use error::{ErrorKind, SaveError, SaveIssue, SceneError};
pub use migrate::{FORMAT_VERSION, STEPS, Step, migrate_table};
pub use prefab::{Collapsed, FsLoader, PrefabError, PrefabSource, collapse, expand};
pub use read::{Loaded, parse, read_version};
pub use spark_core::doc::{Environment, FileKind, FileNode, SceneFile};
pub use world::{SavedScene, SceneLoad, WorldError, load_scene, save_scene, spawn_prefab};
pub use write::{to_string, to_string_lossy};

/// A file brought to the current format.
#[derive(Clone, Debug, PartialEq)]
pub struct Migrated {
    /// The version the file declared.
    pub from: u32,
    /// The version of `text` ([`FORMAT_VERSION`]).
    pub to: u32,
    /// The new canonical text; the input unchanged when `from == to` (an up-to-date file is never rewritten).
    pub text: String,
}

/// Rewrites a scene or prefab text in the current format (migration steps, then the canonical writer). A file that is
/// already current comes back unchanged.
pub fn migrate_text(input: &str) -> Result<Migrated, SceneError> {
    let loaded = parse(input)?;
    if !loaded.migrated() {
        return Ok(Migrated { from: loaded.version, to: FORMAT_VERSION, text: input.to_string() });
    }
    let text = to_string(&loaded.file).map_err(|e| SceneError {
        kind: ErrorKind::Migration,
        message: e.to_string(),
        line: None,
        column: None,
        context: None,
    })?;
    Ok(Migrated { from: loaded.version, to: FORMAT_VERSION, text })
}

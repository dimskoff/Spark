//! Scenes and prefabs in a [`World`]: load a level, spawn a prefab, save the world as a `.scene`. This is what the Luau
//! `scene.load`, `scene.save` and `spawn("x.prefab")` and the editor use; the pieces ([`expand`], [`collapse`],
//! [`instantiate`], [`to_string`]) stay available for tools that need less.

use std::collections::{HashMap, HashSet};
use std::fmt;

use spark_core::doc::{Environment, FileKind, FileNode, InstantiateError, SceneFile, capture, instantiate, instantiate_replacing};
use spark_core::{NodeId, ObjectId, World};

use crate::error::SaveError;
use crate::prefab::{PrefabError, PrefabSource, collapse, expand};
use crate::write::to_string;

/// Why a scene could not be loaded, spawned or saved. The world is unchanged by a failed load.
#[derive(Clone, Debug, PartialEq)]
pub enum WorldError {
    Prefab(PrefabError),
    Instantiate(InstantiateError),
    /// Something in the world cannot be written to a file (a mesh made by code, texture handles).
    Save(SaveError),
    Message(String),
}

impl fmt::Display for WorldError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WorldError::Prefab(e) => write!(f, "{e}"),
            WorldError::Instantiate(e) => write!(f, "{e}"),
            WorldError::Save(e) => write!(f, "{e}"),
            WorldError::Message(m) => f.write_str(m),
        }
    }
}

impl std::error::Error for WorldError {}

impl From<PrefabError> for WorldError {
    fn from(e: PrefabError) -> Self {
        WorldError::Prefab(e)
    }
}

impl From<InstantiateError> for WorldError {
    fn from(e: InstantiateError) -> Self {
        WorldError::Instantiate(e)
    }
}

/// What [`load_scene`] made.
#[derive(Clone, Debug, PartialEq)]
pub struct SceneLoad {
    /// The root objects, in file order (an instance counts as its root).
    pub roots: Vec<ObjectId>,
    /// Objects created, prefab nodes included.
    pub count: usize,
    /// Parts of the file the engine does not use yet, and ids that were renamed.
    pub warnings: Vec<String>,
}

/// Creates the objects of a `.scene` in `world`.
///
/// - `replace`: the objects that exist are deleted first (in the same all-or-nothing step: a file that cannot be loaded
///   leaves the old scene alone) and the file's environment (sun, ambient, background, fog, sky) is applied. The node
///   ids of the file are kept.
/// - otherwise the objects are added; a node id that is already used in the world is renamed (reported in the warnings)
///   and the environment of the world stays.
pub fn load_scene(world: &mut World, file: &SceneFile, source: &mut dyn PrefabSource, replace: bool) -> Result<SceneLoad, WorldError> {
    if file.kind != FileKind::Scene {
        return Err(WorldError::Message(
            "this file is a prefab, not a scene: spawn it with spawn(\"x.prefab\") or put it in a scene as `prefab = \"x.prefab\"`".into(),
        ));
    }
    let old_roots: Vec<NodeId> = if replace {
        let roots: Vec<ObjectId> = world.scene.iter().filter(|(_, o)| o.parent().is_none()).map(|(id, _)| id).collect();
        roots.into_iter().filter_map(|o| world.node_id(o)).collect()
    } else {
        Vec::new()
    };
    let mut expanded = {
        let w: &World = world;
        expand(file, source, &|id| !replace && w.find_node(id).is_some())?
    };
    let mut warnings = Vec::new();
    if !replace {
        let renamed = rename_taken(&mut expanded, world);
        if renamed > 0 {
            warnings.push(format!("{renamed} node id(s) were already used in the scene and were renamed"));
        }
    }
    let environment = Environment::of(&world.scene);
    let done = instantiate_replacing(&expanded, world, &old_roots)?;
    if !replace {
        environment.apply_to(&mut world.scene);
    }
    warnings.extend(done.warnings);
    let roots = done.roots.iter().filter_map(|id| world.find_node(id)).collect();
    Ok(SceneLoad { roots, count: done.count, warnings })
}

/// Gives the nodes whose id is already used in `world` a fresh id. Returns how many were renamed.
fn rename_taken(file: &mut SceneFile, world: &mut World) -> usize {
    let clashes: Vec<NodeId> = file.nodes.iter().filter(|n| world.find_node(&n.id).is_some()).map(|n| n.id.clone()).collect();
    if clashes.is_empty() {
        return 0;
    }
    let mut used: HashSet<NodeId> = file.nodes.iter().map(|n| n.id.clone()).collect();
    let mut renamed: HashMap<NodeId, NodeId> = HashMap::new();
    for old in clashes {
        let new = world.fresh_node_id(&|c| used.contains(c));
        used.insert(new.clone());
        renamed.insert(old, new);
    }
    for n in &mut file.nodes {
        if let Some(new) = renamed.get(&n.id) {
            n.id = new.clone();
        }
        if let Some(new) = n.parent.as_ref().and_then(|p| renamed.get(p)) {
            n.parent = Some(new.clone());
        }
        if let Some(m) = &mut n.member {
            if let Some(new) = renamed.get(&m.instance) {
                m.instance = new.clone();
            }
        }
    }
    renamed.len()
}

/// Creates an instance of the prefab `path` (as written in a scene: `"prefabs/door.prefab"`) and returns its root.
/// The prefab's nodes get fresh ids; the instance is remembered, so [`save_scene`] writes it as `prefab = "..."`.
pub fn spawn_prefab(world: &mut World, path: &str, source: &mut dyn PrefabSource) -> Result<(ObjectId, Vec<String>), WorldError> {
    let id = world.fresh_node_id(&|_| false);
    let mut node = FileNode::new(id.clone());
    node.components.clear();
    node.prefab = Some(path.to_string());
    let mut file = SceneFile::new(FileKind::Scene);
    file.nodes.push(node);
    let expanded = {
        let w: &World = world;
        expand(&file, source, &|c| w.find_node(c).is_some())?
    };
    let environment = Environment::of(&world.scene);
    let done = instantiate(&expanded, world)?;
    environment.apply_to(&mut world.scene);
    let root = world.find_node(&id).ok_or_else(|| WorldError::Message(format!("prefab '{path}': the instance was not created")))?;
    Ok((root, done.warnings))
}

/// A world as scene text.
#[derive(Clone, Debug, PartialEq)]
pub struct SavedScene {
    pub text: String,
    /// Prefab changes a v1 file cannot hold (see [`Collapsed::warnings`](crate::prefab::Collapsed)); the prefab's
    /// version is what the file says.
    pub warnings: Vec<String>,
}

/// The whole world as the text of a `.scene`: every object, the environment, and prefab instances written as instances
/// with only what differs from their prefab. Fails when something cannot be saved (a mesh made by `Mesh.custom`, texture
/// handles), naming the node.
pub fn save_scene(world: &mut World, source: &mut dyn PrefabSource) -> Result<SavedScene, WorldError> {
    let captured = capture(world);
    let collapsed = collapse(&captured, source)?;
    let text = to_string(&collapsed.file).map_err(WorldError::Save)?;
    Ok(SavedScene { text, warnings: collapsed.warnings })
}

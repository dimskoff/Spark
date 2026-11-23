//! The scene document: the format-independent model of a `.scene` / `.prefab` file (ADR 0003), plus
//! [`capture`] (world -> document) and [`instantiate`] (document -> world).
//!
//! The document is **flat**, like the file: a list of [`FileNode`]s in depth-first pre-order, each naming its
//! parent by id. Components are kept as reflection [`Value`]s in their file/inspector form (rotations as Euler angles
//! in degrees, spot angles in radians as the registry stores them; the TOML crate converts units and spellings), so
//! the document can carry what the engine does not interpret yet (`camera`, `script`, prefab overrides) and a file can
//! be read, validated and written without a `World`. The exact, typed subtree form of [`crate::command`]
//! ([`crate::SceneDoc`], [`crate::NodeDoc`]) stays what in-process undo uses; `instantiate` goes through it.
//!
//! ```
//! use spark_core::{World, doc::{capture, instantiate}};
//! use spark_reflect::Value;
//!
//! let mut w = World::new();
//! let a = w.spawn_empty();
//! w.scene[a].name = "Door".into();
//! let file = capture(&mut w);
//! assert_eq!(file.nodes.len(), 1);
//! assert_eq!(file.nodes[0].name, "Door");
//!
//! let mut other = World::new();
//! instantiate(&file, &mut other).unwrap();
//! assert_eq!(capture(&mut other), file);
//! ```
//!
//! Nothing here parses or prints text and there is no serde: that is `spark-scene`.

use std::collections::HashMap;
use std::fmt;

use spark_reflect::{FieldFlags, FieldType, ReflectEnum, ReflectError, TypeInfo, Unit, Value, did_you_mean};

use crate::color::Color;
use crate::command::{Command, CommandError, MeshDoc, MeshState, NodeDoc, SceneDoc};
use crate::mesh_source::MeshSource;
use crate::node_id::NodeId;
use crate::registry::registry;
use crate::scene::{Fog, Object, ObjectId, Scene, ScriptComponent, Sky, Sun};
use crate::world::World;

/// Registry components whose file form is a plain map instead of a table of declared fields: `script` (free-form
/// props, [`ScriptComponent`]). A document keeps it as a plain map and writes it back unchanged; [`instantiate`]
/// attaches it. (`tags` is the plain key `FileNode::tags`.)
pub const RESERVED_COMPONENTS: &[&str] = &["script"];

/// Every component name a file may use, in canonical order (the registry's).
pub fn component_names() -> Vec<&'static str> {
    registry().components.iter().map(|c| c.name).collect()
}

/// Position of a component in the canonical order (unknown names last).
pub fn component_rank(name: &str) -> usize {
    component_names().iter().position(|n| *n == name).unwrap_or(usize::MAX)
}

/// `.scene` (a level) or `.prefab` (a reusable subtree): the header table that is written.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileKind {
    Scene,
    Prefab,
}

impl FileKind {
    /// Name of the header table: `scene` / `prefab`.
    pub fn header(self) -> &'static str {
        match self {
            FileKind::Scene => "scene",
            FileKind::Prefab => "prefab",
        }
    }
}

/// Scene-level lighting and background (`[environment]`). Today these live on [`Scene`], not on objects.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Environment {
    pub ambient: Color,
    pub background: Color,
    pub sun: Sun,
    pub fog: Option<Fog>,
    pub sky: Option<Sky>,
}

impl Default for Environment {
    /// The environment of a new [`Scene`]. Changing it is a breaking change of the file format (ADR 0003).
    fn default() -> Self {
        Self::of(&Scene::new())
    }
}

impl Environment {
    pub fn of(scene: &Scene) -> Self {
        Self { ambient: scene.ambient, background: scene.background, sun: scene.sun, fog: scene.fog, sky: scene.sky }
    }

    pub fn apply_to(&self, scene: &mut Scene) {
        scene.ambient = self.ambient;
        scene.background = self.background;
        scene.sun = self.sun;
        scene.fog = self.fog;
        scene.sky = self.sky;
    }
}

/// One `[[node]]` of a file.
#[derive(Clone, Debug, PartialEq)]
pub struct FileNode {
    /// Unique in the file.
    pub id: NodeId,
    /// `None` = a root.
    pub parent: Option<NodeId>,
    pub name: String,
    pub visible: bool,
    /// Prefab instance (`prefab = "prefabs/door.prefab"`). In a file as written this node is the instance and its
    /// components and `overrides` are what differs from the prefab; after expanding (`spark_scene::expand`) it is the
    /// root of the instance with everything applied, and its [`FileNode::member`] is set.
    pub prefab: Option<String>,
    /// A child added in a scene that attaches inside a prefab instance: `parent` is the instance and this is the id of
    /// the node of the prefab (`parent = "door1/leaf"` in the file). `None` = attaches to `parent` itself.
    pub parent_in: Option<NodeId>,
    /// Set on the nodes of an expanded prefab instance (and by [`capture`] on the live ones): which instance and which
    /// node of the prefab this is. Never written to a file.
    pub member: Option<PrefabMember>,
    /// The `tags` list (the `tags` component of the object).
    pub tags: Vec<String>,
    /// Present components by registry name (`transform`, `mesh`, `material`, `body`, `light`) or reserved name, as
    /// complete values (every field there), in canonical order ([`component_rank`]). The mesh is a recipe:
    /// a [`MeshSource`] value that may carry a `lod` list (see [`mesh_to_value`]).
    pub components: Vec<(String, Value)>,
    /// Prefab overrides: `"<node in the prefab>/<component>.<field>"` -> value in the same form as the component's
    /// fields (a color is a `Color`, a rotation is in degrees, ...), or `"<node>/<component>"` -> the whole component.
    /// `<component>` may be `node` for the `name` and `visible` of a prefab node. Applied by `spark_scene::expand`.
    pub overrides: Vec<(String, Value)>,
}

/// Which node of which prefab instance an object is (see [`FileNode::member`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrefabMember {
    /// Id (in the document or the world) of the instance's root node.
    pub instance: NodeId,
    /// Id of the node inside the prefab file.
    pub node: NodeId,
}

/// A live prefab instance: remembered by the [`World`] so that a save can compare it with its prefab.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrefabInstance {
    /// The prefab file, as written in the scene (`"prefabs/door.prefab"`).
    pub path: String,
    /// `(node id in the prefab, node id in the world)` for every node of the instance, the root first.
    pub members: Vec<(NodeId, NodeId)>,
}

/// The prefab instances of a [`World`], by the world id of their root. Entries are validated against the scene on every
/// read (an instance whose root was destroyed is gone), like the id table.
#[derive(Clone, Debug, Default)]
pub struct PrefabTable {
    map: HashMap<NodeId, PrefabInstance>,
}

impl PrefabTable {
    pub(crate) fn clear(&mut self) {
        self.map.clear();
    }
}

impl World {
    /// Remembers an instance made from `path` (see [`instantiate`]); `members[0]` is the root.
    pub fn register_prefab_instance(&mut self, root: NodeId, instance: PrefabInstance) {
        if self.prefabs.map.len() > 32 {
            let dead: Vec<NodeId> = self.prefabs.map.keys().filter(|k| self.find_node(k).is_none()).cloned().collect();
            for k in dead {
                self.prefabs.map.remove(&k);
            }
        }
        self.prefabs.map.insert(root, instance);
    }

    /// The live prefab instances: `(root id, instance)`, in no particular order.
    pub fn prefab_instances(&self) -> Vec<(&NodeId, &PrefabInstance)> {
        self.prefabs.map.iter().filter(|(k, _)| self.find_node(k).is_some()).collect()
    }

    /// The instance whose root has this node id.
    pub fn prefab_instance(&self, root: &NodeId) -> Option<&PrefabInstance> {
        self.find_node(root).and_then(|_| self.prefabs.map.get(root))
    }
}

impl FileNode {
    /// A visible, unnamed node with the always-present components at their defaults.
    pub fn new(id: NodeId) -> Self {
        let o = Object::empty();
        let components = registry()
            .components
            .iter()
            .filter(|c| c.required)
            .filter_map(|c| (c.get)(&o).map(|v| (c.name.to_string(), round_angles(c.info(), strip_transient(c.info(), v, false)))))
            .collect();
        Self {
            id,
            parent: None,
            name: String::new(),
            visible: true,
            prefab: None,
            parent_in: None,
            member: None,
            tags: Vec::new(),
            components,
            overrides: Vec::new(),
        }
    }

    pub fn component(&self, name: &str) -> Option<&Value> {
        self.components.iter().find(|(n, _)| n == name).map(|(_, v)| v)
    }

    /// Adds or replaces a component, keeping the canonical order.
    pub fn set_component(&mut self, name: &str, value: Value) {
        match self.components.iter_mut().find(|(n, _)| n == name) {
            Some(slot) => slot.1 = value,
            None => {
                self.components.push((name.to_string(), value));
                self.sort_components();
            }
        }
    }

    pub fn sort_components(&mut self) {
        self.components.sort_by_key(|(n, _)| component_rank(n));
    }
}

/// A whole `.scene` or `.prefab`.
#[derive(Clone, Debug, PartialEq)]
pub struct SceneFile {
    pub kind: FileKind,
    /// `name` of the header table (empty = none).
    pub name: String,
    /// The leading `# ...` comment block, one entry per line, kept verbatim (the only comments that survive a save).
    pub comment: Vec<String>,
    /// Scenes only; a prefab has none.
    pub environment: Environment,
    /// Depth-first pre-order (parent before children); siblings keep their order.
    pub nodes: Vec<FileNode>,
}

impl SceneFile {
    pub fn new(kind: FileKind) -> Self {
        Self { kind, name: String::new(), comment: Vec::new(), environment: Environment::default(), nodes: Vec::new() }
    }

    pub fn node(&self, id: &NodeId) -> Option<&FileNode> {
        self.nodes.iter().find(|n| &n.id == id)
    }

    /// Duplicate ids, parents that do not exist and parent cycles. Empty = a well-formed tree.
    pub fn check_structure(&self) -> Vec<StructureIssue> {
        let mut issues = Vec::new();
        // Each id maps to its earliest node; later nodes with the same id are the duplicates.
        let mut earliest: HashMap<&NodeId, usize> = HashMap::new();
        for (i, n) in self.nodes.iter().enumerate() {
            if earliest.contains_key(&n.id) {
                issues.push(StructureIssue { index: i, error: StructureError::DuplicateId(n.id.clone()) });
            } else {
                earliest.insert(&n.id, i);
            }
        }
        for (i, n) in self.nodes.iter().enumerate() {
            if let Some(p) = &n.parent {
                if !earliest.contains_key(p) {
                    issues
                        .push(StructureIssue { index: i, error: StructureError::DanglingParent { node: n.id.clone(), parent: p.clone() } });
                }
            }
        }
        for (i, n) in self.nodes.iter().enumerate() {
            // A node is part of a cycle when walking up its parents leads back to it; report the first one only.
            let mut cur = n.parent.as_ref().and_then(|p| earliest.get(p).copied());
            let mut least = i;
            let mut steps = 0;
            while let Some(c) = cur {
                if c == i {
                    if least == i {
                        issues.push(StructureIssue { index: i, error: StructureError::Cycle(n.id.clone()) });
                    }
                    break;
                }
                least = least.min(c);
                steps += 1;
                if steps > self.nodes.len() {
                    break;
                }
                cur = self.nodes[c].parent.as_ref().and_then(|p| earliest.get(p).copied());
            }
        }
        issues.sort_by_key(|i| i.index);
        issues
    }
}

/// A problem with the shape of the node list; `index` is the position in [`SceneFile::nodes`].
#[derive(Clone, Debug, PartialEq)]
pub struct StructureIssue {
    pub index: usize,
    pub error: StructureError,
}

#[derive(Clone, Debug, PartialEq)]
pub enum StructureError {
    /// Two nodes have this id (reported on the later one).
    DuplicateId(NodeId),
    DanglingParent {
        node: NodeId,
        parent: NodeId,
    },
    /// The node is its own ancestor.
    Cycle(NodeId),
}

impl fmt::Display for StructureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StructureError::DuplicateId(id) => write!(f, "duplicate node id '{id}': ids must be unique in a file"),
            StructureError::DanglingParent { node, parent } => write!(f, "node '{node}' has parent '{parent}', but no node has that id"),
            StructureError::Cycle(id) => write!(f, "node '{id}' is its own ancestor (the parent links form a cycle)"),
        }
    }
}

impl std::error::Error for StructureError {}

// ---------------------------------------------------------------------------------------------------------
// Components as values

/// A component that cannot be read from a value.
#[derive(Clone, Debug, PartialEq)]
pub enum ComponentError {
    /// Not a registry component and not reserved.
    Unknown { name: String, suggestions: Vec<&'static str> },
    /// A field is wrong (type, range, unknown name, runtime-only).
    Invalid(ReflectError),
}

impl fmt::Display for ComponentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ComponentError::Unknown { name, suggestions } => {
                write!(f, "unknown component '{name}'")?;
                if !suggestions.is_empty() {
                    write!(f, "; did you mean {}?", suggestions.join(" or "))?;
                }
                Ok(())
            }
            ComponentError::Invalid(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for ComponentError {}

/// Removes the runtime-only fields (`transient`) of a component value. With `keep_handles` a handle that is set
/// (an `Asset` value, e.g. `runtime:texture/3`) stays, so that a save can report it.
pub fn strip_transient(info: &TypeInfo, v: Value, keep_handles: bool) -> Value {
    match v {
        Value::Map(pairs) => Value::Map(
            pairs
                .into_iter()
                .filter(|(k, val)| {
                    let transient = info.field(k).is_some_and(|f| f.flags.contains(FieldFlags::TRANSIENT));
                    !transient || (keep_handles && matches!(val, Value::Asset(_)))
                })
                .collect(),
        ),
        other => other,
    }
}

/// Angles in a document have at most 4 decimals of a degree. An Euler angle recovered from a quaternion carries `f32`
/// noise (`29.999994`), and without this a document would not equal the one captured after a save and load, and two
/// saves of the same scene could differ. `-0` becomes `0`.
pub fn round_degrees(x: f32) -> f32 {
    let r = ((f64::from(x) * 1e4).round() / 1e4) as f32;
    if r == 0.0 { 0.0 } else { r }
}

/// Rounds the fields measured in degrees (`unit = "deg"`, i.e. `transform.rotation`) with [`round_degrees`].
fn round_angles(info: &TypeInfo, v: Value) -> Value {
    let Value::Map(pairs) = v else { return v };
    let r = round_degrees;
    Value::Map(
        pairs
            .into_iter()
            .map(|(k, val)| {
                let deg = info.field(&k).is_some_and(|f| f.unit == Some(Unit::Degrees));
                let val = match (deg, val) {
                    (true, Value::Float(x)) => Value::Float(f64::from(r(x as f32))),
                    (true, Value::Vec2(a)) => Value::Vec2(a.map(r)),
                    (true, Value::Vec3(a)) => Value::Vec3(a.map(r)),
                    (true, Value::Vec4(a)) => Value::Vec4(a.map(r)),
                    (_, other) => other,
                };
                (k, val)
            })
            .collect(),
    )
}

/// Whether the field is a handle to something that exists only at run time (no file form).
pub fn is_handle_field(ty: &FieldType) -> bool {
    match ty {
        FieldType::Asset(_) => true,
        FieldType::Option(inner) => is_handle_field(inner),
        _ => false,
    }
}

/// The mesh component as a value: the recipe's enum value, plus a `lod` field (a list of `{distance, mesh}` maps)
/// when there are level-of-detail meshes.
pub fn mesh_to_value(source: &MeshSource, lod: &[(f32, MeshSource)]) -> Value {
    match source.to_value() {
        Value::Enum { variant, mut fields } => {
            if !lod.is_empty() {
                let levels = lod
                    .iter()
                    .map(|(d, s)| Value::Map(vec![("distance".into(), Value::Float(f64::from(*d))), ("mesh".into(), s.to_value())]))
                    .collect();
                fields.push(("lod".into(), Value::List(levels)));
            }
            Value::Enum { variant, fields }
        }
        other => other,
    }
}

/// Reads a mesh component value (see [`mesh_to_value`]): the recipe and the level-of-detail recipes.
pub fn mesh_from_value(v: &Value) -> Result<(MeshSource, Vec<(f32, MeshSource)>), ReflectError> {
    let bad = |path: &str, got: &Value| ReflectError::WrongType {
        path: path.to_string(),
        expected: "list of {distance, mesh}",
        got: got.type_name(),
    };
    let Value::Enum { variant, fields } = v else {
        return MeshSource::from_value(v).map(|s| (s, Vec::new()));
    };
    let recipe = Value::Enum { variant: variant.clone(), fields: fields.iter().filter(|(k, _)| k != "lod").cloned().collect() };
    let source = MeshSource::from_value(&recipe)?;
    let mut lod = Vec::new();
    if let Some((_, levels)) = fields.iter().find(|(k, _)| k == "lod") {
        let Value::List(levels) = levels else { return Err(bad("lod", levels)) };
        for (i, level) in levels.iter().enumerate() {
            let at = format!("lod[{i}]");
            let Value::Map(pairs) = level else { return Err(bad(&at, level)) };
            if let Some((k, _)) = pairs.iter().find(|(k, _)| k != "distance" && k != "mesh") {
                let candidates = ["distance", "mesh"];
                return Err(ReflectError::UnknownField {
                    ty: "lod level",
                    path: format!("{at}.{k}"),
                    suggestions: did_you_mean(k, candidates),
                });
            }
            let distance = match level.field("distance") {
                Some(Value::Float(d)) if d.is_finite() && *d >= 0.0 => *d as f32,
                Some(Value::Int(d)) if *d >= 0 => *d as f32,
                Some(Value::Float(d)) => {
                    return Err(if d.is_finite() {
                        ReflectError::OutOfRange { path: format!("{at}.distance"), value: *d, min: 0.0, max: f64::from(f32::MAX) }
                    } else {
                        ReflectError::NotFinite { path: format!("{at}.distance") }
                    });
                }
                Some(Value::Int(d)) => {
                    return Err(ReflectError::OutOfRange {
                        path: format!("{at}.distance"),
                        value: *d as f64,
                        min: 0.0,
                        max: f64::from(f32::MAX),
                    });
                }
                Some(other) => {
                    return Err(ReflectError::WrongType { path: format!("{at}.distance"), expected: "float", got: other.type_name() });
                }
                None => return Err(ReflectError::WrongType { path: format!("{at}.distance"), expected: "float", got: "none" }),
            };
            let mesh =
                level.field("mesh").ok_or(ReflectError::WrongType { path: format!("{at}.mesh"), expected: "mesh recipe", got: "none" })?;
            lod.push((distance, MeshSource::from_value(mesh).map_err(|e| e.at(&format!("{at}.mesh")))?));
        }
    }
    Ok((source, lod))
}

/// Validates a component value against the registry and returns it complete and normalized (missing fields filled
/// with the defaults, integers widened where a float is expected, runtime-only fields removed). Reserved components
/// (`camera`, `script`) are only required to be maps. This is what both reading a file and [`instantiate`] use.
pub fn normalize_component(name: &str, v: &Value) -> Result<Value, ComponentError> {
    let reg = registry();
    if name == "script" {
        return ScriptComponent::from_value(v).map(|s| s.to_value()).map_err(ComponentError::Invalid);
    }
    let Some(info) = reg.component(name) else {
        return Err(ComponentError::Unknown { name: name.to_string(), suggestions: did_you_mean(name, component_names()) });
    };
    if name == "mesh" {
        let (source, lod) = mesh_from_value(v).map_err(ComponentError::Invalid)?;
        return Ok(mesh_to_value(&source, &lod));
    }
    let mut o = Object::empty();
    let res = if info.required { (info.set)(&mut o, "", v.clone()) } else { (info.add)(&mut o, Some(v)) };
    res.map_err(ComponentError::Invalid)?;
    let full = (info.get)(&o).expect("the component was just stored");
    Ok(round_angles(info.info(), strip_transient(info.info(), full, false)))
}

// ---------------------------------------------------------------------------------------------------------
// World -> document

/// The whole scene as a document: environment and every object (all roots, in slot order, depth-first), with the
/// node id of each (assigned now for objects that had none). Runtime-only parts are kept as handles
/// (`runtime:...` assets, [`MeshSource::RuntimeOnly`] meshes) so that a save can name them instead of dropping them.
pub fn capture(world: &mut World) -> SceneFile {
    let mut file = SceneFile::new(FileKind::Scene);
    file.environment = Environment::of(&world.scene);
    // Live prefab instances: world node id -> (instance root, node in the prefab), and the path of each root.
    let mut member_of: HashMap<NodeId, PrefabMember> = HashMap::new();
    let mut prefab_path: HashMap<NodeId, String> = HashMap::new();
    for (root, inst) in world.prefab_instances() {
        prefab_path.insert(root.clone(), inst.path.clone());
        for (pnode, wid) in &inst.members {
            if world.find_node(wid).is_some() {
                member_of.insert(wid.clone(), PrefabMember { instance: root.clone(), node: pnode.clone() });
            }
        }
    }
    let roots: Vec<ObjectId> = world.scene.iter().filter(|(_, o)| o.parent().is_none()).map(|(id, _)| id).collect();
    let mut stack: Vec<ObjectId> = roots.into_iter().rev().collect();
    while let Some(oid) = stack.pop() {
        let id = world.node_id(oid).expect("the object is alive");
        let obj = world.scene[oid].clone();
        let parent = obj.parent().and_then(|p| world.node_id(p));
        stack.extend(obj.child_ids().iter().rev().copied());
        let mut node = FileNode::new(id);
        node.parent = parent;
        node.name = obj.name.clone();
        node.visible = obj.visible;
        node.member = member_of.get(&node.id).cloned();
        node.prefab = prefab_path.get(&node.id).cloned();
        node.components.clear();
        for c in registry().components {
            if c.name == "mesh" {
                if let Some(ms) = world.mesh_state(&obj) {
                    let lod: Vec<(f32, MeshSource)> = ms.lod.iter().map(|(d, m)| (*d, m.source.clone())).collect();
                    node.components.push(("mesh".into(), mesh_to_value(&ms.mesh.source, &lod)));
                }
            } else if let Some(v) = (c.get)(&obj) {
                let v = if RESERVED_COMPONENTS.contains(&c.name) { v } else { round_angles(c.info(), strip_transient(c.info(), v, true)) };
                node.components.push((c.name.to_string(), v));
            }
        }
        node.tags = obj.tags.clone();
        file.nodes.push(node);
    }
    file
}

// ---------------------------------------------------------------------------------------------------------
// Document -> world

/// What [`instantiate`] did.
#[derive(Clone, Debug, PartialEq)]
pub struct Instantiated {
    /// Ids of the root nodes, in file order.
    pub roots: Vec<NodeId>,
    /// Number of objects created.
    pub count: usize,
    /// Parts of the document that were ignored because they were not expanded (prefab instances: see `spark_scene::expand`). A `script` is attached (the script host
    /// starts it on its next frame).
    pub warnings: Vec<String>,
    /// Removes what was created (the environment is not restored).
    pub undo: Command,
}

/// Why a document could not be instantiated. The world is unchanged.
#[derive(Clone, Debug, PartialEq)]
pub enum InstantiateError {
    Structure(StructureError),
    Component {
        node: NodeId,
        component: String,
        error: ComponentError,
    },
    /// A node id is already used in the world, a mesh recipe cannot be built, ...
    Spawn(CommandError),
}

impl fmt::Display for InstantiateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            InstantiateError::Structure(e) => write!(f, "{e}"),
            InstantiateError::Component { node, component, error } => write!(f, "node '{node}', {component}: {error}"),
            InstantiateError::Spawn(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for InstantiateError {}

/// Creates the objects of `doc` in `world` (additive: existing objects stay; use a fresh or cleared world to
/// replace). All or nothing. A scene file also sets the environment. Node ids are kept, so an id that is already used
/// in the world is an error.
pub fn instantiate(doc: &SceneFile, world: &mut World) -> Result<Instantiated, InstantiateError> {
    instantiate_replacing(doc, world, &[])
}

/// [`instantiate`] that first deletes the subtrees rooted at `replace` (the nodes of the scene being replaced), in the
/// same all-or-nothing step: when the document cannot be instantiated the old objects are still there. The ids of the
/// deleted nodes are free for the document to use.
pub fn instantiate_replacing(doc: &SceneFile, world: &mut World, replace: &[NodeId]) -> Result<Instantiated, InstantiateError> {
    if let Some(issue) = doc.check_structure().into_iter().next() {
        return Err(InstantiateError::Structure(issue.error));
    }
    let mut warnings = Vec::new();
    let mut docs: Vec<Option<NodeDoc>> = Vec::with_capacity(doc.nodes.len());
    for n in &doc.nodes {
        docs.push(Some(node_doc(n, &mut warnings)?));
    }
    // Tree shape: children lists in file order.
    let index: HashMap<&NodeId, usize> = doc.nodes.iter().enumerate().map(|(i, n)| (&n.id, i)).collect();
    let parent_of: Vec<Option<usize>> = doc.nodes.iter().map(|n| n.parent.as_ref().map(|p| index[p])).collect();
    let mut kids: Vec<Vec<usize>> = vec![Vec::new(); doc.nodes.len()];
    let mut roots = Vec::new();
    for (i, p) in parent_of.iter().enumerate() {
        match p {
            Some(p) => kids[*p].push(i),
            None => roots.push(i),
        }
    }
    let mut order = Vec::with_capacity(doc.nodes.len());
    let mut stack: Vec<usize> = roots.iter().rev().copied().collect();
    while let Some(i) = stack.pop() {
        order.push(i);
        stack.extend(kids[i].iter().rev().copied());
    }
    // Children come after their parent in `order`: fold from the back (they arrive reversed).
    let mut root_docs = Vec::new();
    for &i in order.iter().rev() {
        let mut d = docs[i].take().expect("each node is folded once");
        d.children.reverse();
        match parent_of[i] {
            Some(p) => docs[p].as_mut().expect("the parent is folded later").children.push(d),
            None => root_docs.push(d),
        }
    }
    root_docs.reverse();
    let root_ids: Vec<NodeId> = root_docs.iter().filter_map(|d| d.id.clone()).collect();
    let mut steps: Vec<Command> = replace.iter().map(|n| Command::Delete { node: n.clone() }).collect();
    steps.extend(root_docs.into_iter().map(|d| Command::Spawn { subtree: SceneDoc::new(d), parent: None, index: None }));
    let applied = world.apply(&Command::Batch(steps)).map_err(InstantiateError::Spawn)?;
    if doc.kind == FileKind::Scene {
        doc.environment.apply_to(&mut world.scene);
    }
    // Expanded prefab instances are remembered, so that a save can write them as instances again.
    for n in &doc.nodes {
        let Some(path) = &n.prefab else { continue };
        if !n.member.as_ref().is_some_and(|m| m.instance == n.id) {
            continue;
        }
        let members = doc
            .nodes
            .iter()
            .filter_map(|m| m.member.as_ref().filter(|mm| mm.instance == n.id).map(|mm| (mm.node.clone(), m.id.clone())))
            .collect();
        world.register_prefab_instance(n.id.clone(), PrefabInstance { path: path.clone(), members });
    }
    Ok(Instantiated { roots: root_ids, count: doc.nodes.len(), warnings, undo: applied.inverse })
}

fn node_doc(n: &FileNode, warnings: &mut Vec<String>) -> Result<NodeDoc, InstantiateError> {
    let fail = |component: &str, error: ComponentError| InstantiateError::Component {
        node: n.id.clone(),
        component: component.to_string(),
        error,
    };
    let mut d = NodeDoc { id: Some(n.id.clone()), name: n.name.clone(), visible: n.visible, ..NodeDoc::default() };
    let mut o = Object::empty();
    let reg = registry();
    for (name, v) in &n.components {
        if name == "script" {
            d.script = Some(ScriptComponent::from_value(v).map_err(|e| fail(name, ComponentError::Invalid(e)))?);
            continue;
        }
        let Some(info) = reg.component(name) else {
            return Err(fail(name, ComponentError::Unknown { name: name.clone(), suggestions: did_you_mean(name, component_names()) }));
        };
        if name == "mesh" {
            let (source, lod) = mesh_from_value(v).map_err(|e| fail(name, ComponentError::Invalid(e)))?;
            d.mesh = Some(MeshState {
                mesh: MeshDoc::from_source(source),
                lod: lod.into_iter().map(|(dist, s)| (dist, MeshDoc::from_source(s))).collect(),
            });
            continue;
        }
        let res = if info.required { (info.set)(&mut o, "", v.clone()) } else { (info.add)(&mut o, Some(v)) };
        res.map_err(|e| fail(name, ComponentError::Invalid(e)))?;
    }
    d.transform = o.transform;
    d.material = o.material;
    d.body = o.body;
    d.light = o.light;
    d.camera = o.camera;
    if let Some(s) = &d.script {
        if s.path.trim().is_empty() {
            return Err(fail(
                "script",
                ComponentError::Invalid(ReflectError::WrongType { path: "path".into(), expected: "a file path", got: "empty string" }),
            ));
        }
    }
    if !n.tags.is_empty() {
        let mut probe = Object::empty();
        probe.set_tags(&n.tags).map_err(|m| InstantiateError::Spawn(CommandError::Invalid(format!("node '{}', tags: {m}", n.id))))?;
        d.tags = probe.tags;
    }
    if let (Some(p), None) = (&n.prefab, &n.member) {
        warnings.push(format!("node '{}': the prefab instance '{p}' was not expanded (spark_scene::expand), so the node is empty", n.id));
        if !n.overrides.is_empty() {
            warnings.push(format!("node '{}': prefab overrides were ignored (the instance was not expanded)", n.id));
        }
    }
    if n.parent_in.is_some() && n.member.is_none() {
        warnings
            .push(format!("node '{}': `parent_in` needs an expanded prefab instance; the node was attached to the instance root", n.id));
    }
    Ok(d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::glam::{Quat, Vec3};
    use crate::{Body, Light, Material};

    fn nid(s: &str) -> NodeId {
        NodeId::new(s).unwrap()
    }

    #[test]
    fn capture_instantiate_capture_is_stable() {
        let mut w = World::new();
        let cube = w.assets.add_mesh_from(&MeshSource::Cube { size: 2.0 }).unwrap();
        let a = w.spawn(cube, Material::color(Color::RED));
        w.scene[a].name = "A".into();
        w.scene[a].transform.position = Vec3::new(1.0, 2.0, 3.0);
        w.scene[a].transform.rotation = Quat::from_rotation_y(90f32.to_radians());
        w.scene[a].body = Some(Body::default());
        let b = w.spawn_empty();
        w.scene[b].light = Some(Light::default());
        w.scene[b].visible = false;
        w.scene.set_parent(b, Some(a)).unwrap();
        let c = w.spawn_empty();
        w.scene[c].name = "C".into();

        let doc = capture(&mut w);
        assert_eq!(doc.nodes.len(), 3);
        assert_eq!(doc.nodes[1].parent.as_ref(), Some(&doc.nodes[0].id), "pre-order: the child follows its parent");
        assert!(doc.check_structure().is_empty());

        let mut w2 = World::new();
        let r = instantiate(&doc, &mut w2).unwrap();
        assert_eq!(r.roots, vec![doc.nodes[0].id.clone(), doc.nodes[2].id.clone()]);
        assert!(r.warnings.is_empty());
        assert_eq!(capture(&mut w2), doc);

        // The undo removes what was created.
        w2.apply(&r.undo).unwrap();
        assert!(w2.scene.is_empty());
    }

    #[test]
    fn degrees_are_rounded_to_four_decimals() {
        assert_eq!(round_degrees(30.000002), 30.0);
        assert_eq!(round_degrees(-0.000001), 0.0);
        assert_eq!(round_degrees(-20.00001), -20.0);
        assert_eq!(round_degrees(12.34567), 12.3457);
    }

    #[test]
    fn structure_problems_are_found() {
        let node = |id: &str, parent: Option<&str>| {
            let mut n = FileNode::new(nid(id));
            n.parent = parent.map(nid);
            n
        };
        let mut f = SceneFile::new(FileKind::Scene);
        f.nodes = vec![node("a", None), node("b", Some("a")), node("a", None), node("c", Some("zz"))];
        let issues = f.check_structure();
        assert_eq!(issues.iter().map(|i| i.index).collect::<Vec<_>>(), vec![2, 3]);
        assert!(matches!(issues[0].error, StructureError::DuplicateId(_)));
        assert!(matches!(issues[1].error, StructureError::DanglingParent { .. }));

        f.nodes = vec![node("a", Some("b")), node("b", Some("a")), node("c", Some("a"))];
        let issues = f.check_structure();
        assert_eq!(issues.len(), 1, "one report per cycle, none for the nodes below it");
        assert!(matches!(&issues[0].error, StructureError::Cycle(id) if id.as_str() == "a"));
        let mut w = World::new();
        assert!(matches!(instantiate(&f, &mut w), Err(InstantiateError::Structure(_))));
        assert!(w.scene.is_empty());
    }

    #[test]
    fn components_are_validated_and_completed() {
        let v = normalize_component("body", &Value::Map(vec![("mass".into(), Value::Int(4))])).unwrap();
        assert_eq!(v.field("mass"), Some(&Value::Float(4.0)));
        assert!(v.field("velocity").is_none(), "runtime-only fields are not part of a document");
        let e = normalize_component("body", &Value::Map(vec![("masss".into(), Value::Float(1.0))])).unwrap_err();
        assert!(e.to_string().contains("did you mean `mass`"), "{e}");
        let e = normalize_component("bodyy", &Value::Map(vec![])).unwrap_err();
        assert!(e.to_string().contains("did you mean body"), "{e}");
        assert!(normalize_component("camera", &Value::Map(vec![])).is_ok());
        assert!(normalize_component("camera", &Value::Int(1)).is_err());
    }

    #[test]
    fn runtime_only_parts_stay_visible_in_a_capture() {
        let mut w = World::new();
        let id = w.add_mesh(crate::MeshData::cube(1.0));
        w.spawn(id, Material::default());
        let doc = capture(&mut w);
        let mesh = doc.nodes[0].component("mesh").unwrap();
        assert!(matches!(mesh, Value::Enum { variant, .. } if variant == "runtime"));
    }

    #[test]
    fn duplicate_ids_in_the_world_are_rejected() {
        let mut w = World::new();
        w.spawn_empty();
        let doc = capture(&mut w);
        assert!(matches!(instantiate(&doc, &mut w), Err(InstantiateError::Spawn(CommandError::DuplicateNode(_)))));
        assert_eq!(w.scene.len(), 1);
    }

    #[test]
    fn tags_and_camera_are_instantiated() {
        let mut f = SceneFile::new(FileKind::Scene);
        let mut n = FileNode::new(nid("a"));
        n.tags = vec!["x".into()];
        n.set_component("camera", Value::Map(vec![("fov".into(), Value::Float(60.0))]));
        f.nodes.push(n);
        let mut w = World::new();
        let r = instantiate(&f, &mut w).unwrap();
        assert!(r.warnings.is_empty(), "{:?}", r.warnings);
        assert_eq!(w.scene.len(), 1);
        let id = w.scene.find("").unwrap();
        assert_eq!(w.scene[id].tags(), ["x"]);
        assert_eq!(w.scene[id].camera().unwrap().fov, 60.0);
        assert_eq!(capture(&mut w).nodes[0].tags, ["x"]);
    }

    #[test]
    fn scripts_are_attached_captured_and_not_warned_about() {
        let mut f = SceneFile::new(FileKind::Scene);
        for (id, speed) in [("a", 2), ("b", 3)] {
            let mut n = FileNode::new(nid(id));
            let s = ScriptComponent { path: "src/door.luau".into(), props: vec![("speed".into(), Value::Int(speed))] };
            n.set_component("script", s.to_value());
            f.nodes.push(n);
        }
        let mut w = World::new();
        let r = instantiate(&f, &mut w).unwrap();
        assert!(r.warnings.is_empty(), "{:?}", r.warnings);
        let order: Vec<u32> = w.scene.scripted().iter().map(|(_, o)| o.index()).collect();
        assert_eq!(order.len(), 2);
        assert_eq!(capture(&mut w), f);
        // A misspelled key is an error with a suggestion.
        let mut bad = f.clone();
        bad.nodes[0].set_component("script", Value::Map(vec![("pth".into(), Value::Str("x".into()))]));
        let e = instantiate(&bad, &mut World::new()).unwrap_err().to_string();
        assert!(e.contains("path"), "{e}");
        // Undo removes the objects and their script entries.
        w.apply(&r.undo).unwrap();
        assert!(w.scene.scripted().is_empty());
    }

    #[test]
    fn camera_node_follows_commands_and_undo() {
        use crate::camera::CameraNode;
        let mut w = World::new();
        let free = w.scene.camera;
        let a = w.spawn_empty();
        let id = w.node_id(a).unwrap();
        w.scene[a].transform.position = Vec3::new(1.0, 2.0, 3.0);
        // No camera node: the free camera is untouched by commands.
        w.apply(&Command::SetProperty { node: id.clone(), path: "transform.position".into(), value: Value::Vec3([9.0, 9.0, 9.0]) })
            .unwrap();
        assert_eq!(w.scene.camera, free);
        let add = w.apply(&Command::AddComponent { node: id.clone(), component: "camera".into(), value: None }).unwrap();
        assert_eq!(w.scene.camera.position, Vec3::new(9.0, 9.0, 9.0));
        let set = Command::SetProperty { node: id.clone(), path: "camera.fov".into(), value: Value::Float(33.0) };
        let undo = w.apply(&set).unwrap().inverse;
        assert_eq!(w.scene.camera.fov, 33.0);
        w.apply(&undo).unwrap();
        assert_eq!(w.scene.camera.fov, CameraNode::default().fov);
        // Writing the render camera moves the node (the Luau `camera` global does this).
        w.scene.camera.position = Vec3::new(4.0, 5.0, 6.0);
        assert!(w.scene.sync_camera_to_node());
        assert_eq!(w.scene[a].transform.position, Vec3::new(4.0, 5.0, 6.0));
        // The first active node wins; deactivating it hands over, removing the last one ends the sync.
        w.apply(&Command::SetProperty { node: id.clone(), path: "camera.active".into(), value: Value::Bool(false) }).unwrap();
        assert!(w.scene.active_camera_node().is_none());
        w.apply(&add.inverse).unwrap();
        assert!(w.scene.camera_nodes().is_empty());
        w.scene.camera.fov = 20.0;
        assert!(!w.scene.sync_camera_to_node());
        assert!(w.scene.get_mut(a).unwrap().camera().is_none());
    }
}

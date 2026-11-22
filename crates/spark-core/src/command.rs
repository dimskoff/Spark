//! Commands: the one way a document changes (ADR 0005, section 2).
//!
//! Every edit of the world made by an editor, an agent or devtools is a [`Command`] applied with
//! [`World::apply`]. Applying returns an [`Applied`] whose `inverse` is a command that restores the previous state
//! (value-based: the inverse of `Delete` re-spawns the captured subtree with the same node ids), so undo is
//! "apply the inverse" and needs no scene snapshot. [`crate::History`] keeps the pairs.
//!
//! Commands address objects by [`NodeId`], never by `ObjectId`. Property paths and values go through the component
//! registry ([`crate::registry`]), so they are validated like in scripts and files.
//!
//! No serde here: the JSON / TOML forms of a command belong to the crates that own those dependencies (devtools,
//! scene files). The enum is the wire format; [`Command::Restore`] and the typed parts of [`NodeDoc`] are exact,
//! in-process forms (a quaternion survives bit for bit, texture handles and velocities are kept) that those crates
//! can encode as they see fit.
//!
//! ```
//! use spark_core::{Command, NodeDoc, SceneDoc, World, glam::Vec3};
//! use spark_reflect::Value;
//!
//! let mut w = World::new();
//! let spawn = Command::Spawn { subtree: SceneDoc::new(NodeDoc::named("Crate")), parent: None, index: None };
//! let id = w.apply(&spawn).unwrap().changed[0].clone();
//! let set = Command::SetProperty { node: id.clone(), path: "transform.position".into(), value: Value::Vec3([1.0, 2.0, 3.0]) };
//! let undo = w.apply(&set).unwrap().inverse;
//! assert_eq!(w.scene[w.find_node(&id).unwrap()].transform.position, Vec3::new(1.0, 2.0, 3.0));
//! w.apply(&undo).unwrap();
//! assert_eq!(w.scene[w.find_node(&id).unwrap()].transform.position, Vec3::ZERO);
//! ```

use std::collections::HashSet;
use std::fmt;

use spark_reflect::{ReflectEnum, ReflectError, Value, did_you_mean, split_path};

use crate::assets::MeshId;
use crate::material::Material;
use crate::mesh::Aabb;
use crate::mesh_source::MeshSource;
use crate::node_id::NodeId;
use crate::physics::Body;
use crate::registry::registry;
use crate::scene::{Light, Object, ObjectId, ScriptComponent, Transform};
use crate::world::World;

/// An edit of the world. See the module docs.
// Commands are built one at a time by editors and agents; boxing the subtree would only make construction clumsy.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    /// Creates a subtree. Ids inside are kept (ids that are `None` are generated and written back into
    /// [`Applied::resolved`], so redo recreates the same ids). `index` is the position among the parent's children;
    /// it is ignored for root nodes (their order is the slot order of the scene).
    Spawn { subtree: SceneDoc, parent: Option<NodeId>, index: Option<usize> },
    /// Deletes a node and its children.
    Delete { node: NodeId },
    /// `path` = `"transform.position"`, `"body.mass"`, `"material.color"`, `"name"`, `"visible"`. The path of a whole
    /// component (`"body"`) replaces it.
    SetProperty { node: NodeId, path: String, value: Value },
    /// Moves a node under another one (`None` = to the root), keeping its local transform. Cycles are rejected.
    /// `index` is the position among the new parent's children once the node is removed from its old place.
    Reparent { node: NodeId, parent: Option<NodeId>, index: Option<usize> },
    /// Adds (or replaces) a component. `None` = defaults. For `mesh` the value is a [`MeshSource`] as a value.
    AddComponent { node: NodeId, component: String, value: Option<Value> },
    /// Removes an optional component.
    RemoveComponent { node: NodeId, component: String },
    /// Puts a component back to a captured state, exactly. This is what the inverse of a transform edit, of a whole
    /// component replacement and of a component removal is; it can also be sent directly.
    Restore { node: NodeId, state: ComponentState },
    /// All or nothing: on the first error the already applied part is rolled back through its inverses.
    Batch(Vec<Command>),
}

impl Command {
    /// Short human description ("Set transform.position"), used as the undo label.
    pub fn label(&self) -> String {
        match self {
            Command::Spawn { subtree, .. } => format!("Create {}", subtree.root.display_name()),
            Command::Delete { node } => format!("Delete {node}"),
            Command::SetProperty { path, .. } => format!("Set {path}"),
            Command::Reparent { .. } => "Reparent".to_string(),
            Command::AddComponent { component, .. } => format!("Add {component}"),
            Command::RemoveComponent { component, .. } => format!("Remove {component}"),
            Command::Restore { state, .. } => format!("Restore {}", state.component()),
            Command::Batch(v) => match v.as_slice() {
                [one] => one.label(),
                _ => format!("{} changes", v.len()),
            },
        }
    }
}

/// What [`World::apply`] did.
#[derive(Clone, Debug, PartialEq)]
pub struct Applied {
    /// Restores the state before the command.
    pub inverse: Command,
    /// Nodes that were created, changed or deleted (for the editor / devtools to refresh).
    pub changed: Vec<NodeId>,
    /// The command with every implicit choice made explicit (generated ids of a `Spawn`); `None` = the command as
    /// given. Redo must use this one.
    pub resolved: Option<Command>,
}

/// Why a command failed. Nothing was changed.
#[derive(Clone, Debug, PartialEq)]
pub enum CommandError {
    NoSuchNode(NodeId),
    /// A `Spawn` would create an id that is already used (in the scene or twice in the subtree).
    DuplicateNode(NodeId),
    /// Not a component of the registry.
    UnknownComponent {
        name: String,
        suggestions: Vec<&'static str>,
    },
    /// The node has no such (optional) component.
    NoSuchComponent {
        node: NodeId,
        component: String,
    },
    /// `transform` and `material` are always there.
    RequiredComponent {
        component: String,
    },
    /// Reparenting would make a node its own ancestor.
    Cycle {
        node: NodeId,
        parent: NodeId,
    },
    /// Path, type, range, read-only: see [`ReflectError`].
    Reflect(ReflectError),
    /// Anything else (a mesh recipe that cannot be built, a bad id).
    Invalid(String),
}

impl CommandError {
    /// Stable machine-readable code.
    pub fn code(&self) -> &'static str {
        match self {
            CommandError::NoSuchNode(_) => "no_such_node",
            CommandError::DuplicateNode(_) => "duplicate_node",
            CommandError::UnknownComponent { .. } => "unknown_component",
            CommandError::NoSuchComponent { .. } => "no_such_component",
            CommandError::RequiredComponent { .. } => "required_component",
            CommandError::Cycle { .. } => "cycle",
            CommandError::Reflect(e) => e.code(),
            CommandError::Invalid(_) => "invalid",
        }
    }
}

impl fmt::Display for CommandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CommandError::NoSuchNode(n) => write!(f, "no node with id '{n}'"),
            CommandError::DuplicateNode(n) => write!(f, "a node with id '{n}' already exists"),
            CommandError::UnknownComponent { name, suggestions } => {
                write!(f, "unknown component '{name}'")?;
                if !suggestions.is_empty() {
                    write!(f, " (did you mean {}?)", suggestions.join(", "))?;
                }
                Ok(())
            }
            CommandError::NoSuchComponent { node, component } => write!(f, "node '{node}' has no '{component}' component"),
            CommandError::RequiredComponent { component } => write!(f, "'{component}' is always present and cannot be removed"),
            CommandError::Cycle { node, parent } => write!(f, "cannot move '{node}' under '{parent}': it would become its own ancestor"),
            CommandError::Reflect(e) => write!(f, "{e:?}"),
            CommandError::Invalid(s) => f.write_str(s),
        }
    }
}

impl std::error::Error for CommandError {}

impl From<ReflectError> for CommandError {
    fn from(e: ReflectError) -> Self {
        CommandError::Reflect(e)
    }
}

// ---------------------------------------------------------------------------------------------------------
// The subtree description (minimal; C-14 expands it into the scene file model)

/// A subtree to create, or a captured one. Typed and exact (not `Value`-based): C-14 adds the conversions.
#[derive(Clone, Debug, PartialEq)]
pub struct SceneDoc {
    pub root: NodeDoc,
}

impl SceneDoc {
    pub fn new(root: NodeDoc) -> Self {
        Self { root }
    }
}

/// One node of a [`SceneDoc`]: the node fields, the components and the children, in order.
#[derive(Clone, Debug, PartialEq)]
pub struct NodeDoc {
    /// `None` = generate one.
    pub id: Option<NodeId>,
    pub name: String,
    pub visible: bool,
    pub transform: Transform,
    pub material: Material,
    pub body: Option<Body>,
    pub light: Option<Light>,
    pub mesh: Option<MeshState>,
    /// The attached script and its props.
    pub script: Option<ScriptComponent>,
    pub children: Vec<NodeDoc>,
}

impl Default for NodeDoc {
    fn default() -> Self {
        Self {
            id: None,
            name: String::new(),
            visible: true,
            transform: Transform::default(),
            material: Material::default(),
            body: None,
            light: None,
            mesh: None,
            script: None,
            children: Vec::new(),
        }
    }
}

impl NodeDoc {
    /// An empty node with a name.
    pub fn named(name: &str) -> Self {
        Self { name: name.to_string(), ..Self::default() }
    }

    fn display_name(&self) -> String {
        match (&self.name[..], &self.id) {
            ("", Some(id)) => id.to_string(),
            ("", None) => "node".to_string(),
            (n, _) => n.to_string(),
        }
    }
}

/// A mesh by recipe, plus the runtime handle it had when captured. `handle` is used when it still points at a mesh
/// with the same recipe (always true within a session, since meshes are never freed individually), which is what
/// makes runtime-only meshes restorable by undo; without a valid handle the recipe is rebuilt.
#[derive(Clone, Debug, PartialEq)]
pub struct MeshDoc {
    pub source: MeshSource,
    pub handle: Option<MeshId>,
}

impl MeshDoc {
    /// A mesh from a recipe only.
    pub fn from_source(source: MeshSource) -> Self {
        Self { source, handle: None }
    }
}

/// The mesh component: the mesh and its level-of-detail meshes (`distance`, mesh).
#[derive(Clone, Debug, PartialEq)]
pub struct MeshState {
    pub mesh: MeshDoc,
    pub lod: Vec<(f32, MeshDoc)>,
}

/// The exact state of one component (or its absence): the argument of [`Command::Restore`].
#[derive(Clone, Debug, PartialEq)]
pub enum ComponentState {
    Transform(Transform),
    Material(Material),
    Body(Option<Body>),
    Light(Option<Light>),
    Mesh(Option<MeshState>),
}

impl ComponentState {
    /// Registry name of the component.
    pub fn component(&self) -> &'static str {
        match self {
            ComponentState::Transform(_) => "transform",
            ComponentState::Material(_) => "material",
            ComponentState::Body(_) => "body",
            ComponentState::Light(_) => "light",
            ComponentState::Mesh(_) => "mesh",
        }
    }

    /// An optional component that is not there.
    pub fn is_absent(&self) -> bool {
        matches!(self, ComponentState::Body(None) | ComponentState::Light(None) | ComponentState::Mesh(None))
    }
}

// ---------------------------------------------------------------------------------------------------------
// Applying

impl World {
    /// Applies a command to the world. On error nothing has changed. See the module docs.
    pub fn apply(&mut self, cmd: &Command) -> Result<Applied, CommandError> {
        match cmd {
            Command::Spawn { subtree, parent, index } => self.apply_spawn(subtree, parent.as_ref(), *index),
            Command::Delete { node } => self.apply_delete(node),
            Command::SetProperty { node, path, value } => self.apply_set(node, path, value),
            Command::Reparent { node, parent, index } => self.apply_reparent(node, parent.as_ref(), *index),
            Command::AddComponent { node, component, value } => self.apply_add(node, component, value.as_ref()),
            Command::RemoveComponent { node, component } => self.apply_remove(node, component),
            Command::Restore { node, state } => {
                let oid = self.node_object(node)?;
                let before = self.component_state(oid, state.component()).expect("every state names a registry component");
                self.apply_state(oid, state.clone())?;
                Ok(applied(Command::Restore { node: node.clone(), state: before }, vec![node.clone()]))
            }
            Command::Batch(cmds) => self.apply_batch(cmds),
        }
    }

    fn node_object(&self, node: &NodeId) -> Result<ObjectId, CommandError> {
        self.find_node(node).ok_or_else(|| CommandError::NoSuchNode(node.clone()))
    }

    fn apply_batch(&mut self, cmds: &[Command]) -> Result<Applied, CommandError> {
        let mut inverses = Vec::with_capacity(cmds.len());
        let mut changed: Vec<NodeId> = Vec::new();
        let mut resolved: Vec<Option<Command>> = Vec::with_capacity(cmds.len());
        for c in cmds {
            match self.apply(c) {
                Ok(a) => {
                    inverses.push(a.inverse);
                    for n in a.changed {
                        if !changed.contains(&n) {
                            changed.push(n);
                        }
                    }
                    resolved.push(a.resolved);
                }
                Err(e) => {
                    for inv in inverses.iter().rev() {
                        let rolled_back = self.apply(inv);
                        debug_assert!(rolled_back.is_ok(), "an inverse command failed during a batch rollback");
                    }
                    return Err(e);
                }
            }
        }
        inverses.reverse();
        let resolved = resolved
            .iter()
            .any(Option::is_some)
            .then(|| Command::Batch(cmds.iter().zip(resolved).map(|(c, r)| r.unwrap_or_else(|| c.clone())).collect()));
        Ok(Applied { inverse: Command::Batch(inverses), changed, resolved })
    }

    // ----- Spawn / Delete -------------------------------------------------------------------------------

    fn apply_spawn(&mut self, subtree: &SceneDoc, parent: Option<&NodeId>, index: Option<usize>) -> Result<Applied, CommandError> {
        let parent_oid = parent.map(|p| self.node_object(p)).transpose()?;
        // Ids: explicit ones must be unique and free, missing ones are generated.
        let mut doc = subtree.clone();
        let mut used = HashSet::new();
        self.check_ids(&doc.root, &mut used)?;
        let mut generated = false;
        self.fill_ids(&mut doc.root, &mut used, &mut generated);

        let mut root = None;
        let built = self.build_node(&doc.root, parent_oid, &mut root);
        let root_oid = match built {
            Ok(o) => o,
            Err(e) => {
                if let Some(r) = root {
                    self.scene.destroy(r);
                }
                return Err(e);
            }
        };
        if let (Some(p), Some(i)) = (parent_oid, index) {
            self.place_child(p, root_oid, i);
        }
        let root_id = doc.root.id.clone().expect("ids were filled");
        let mut changed = Vec::new();
        collect_ids(&doc.root, &mut changed);
        let resolved = generated.then(|| Command::Spawn { subtree: doc, parent: parent.cloned(), index });
        Ok(Applied { inverse: Command::Delete { node: root_id }, changed, resolved })
    }

    fn check_ids(&self, n: &NodeDoc, used: &mut HashSet<NodeId>) -> Result<(), CommandError> {
        if let Some(id) = &n.id {
            if self.find_node(id).is_some() || !used.insert(id.clone()) {
                return Err(CommandError::DuplicateNode(id.clone()));
            }
        }
        n.children.iter().try_for_each(|c| self.check_ids(c, used))
    }

    fn fill_ids(&mut self, n: &mut NodeDoc, used: &mut HashSet<NodeId>, generated: &mut bool) {
        if n.id.is_none() {
            let id = self.fresh_node_id(&|id| used.contains(id));
            used.insert(id.clone());
            n.id = Some(id);
            *generated = true;
        }
        for c in &mut n.children {
            self.fill_ids(c, used, generated);
        }
    }

    /// Creates the object of `n` (ids are filled) and its children. `root` receives the first object created.
    fn build_node(&mut self, n: &NodeDoc, parent: Option<ObjectId>, root: &mut Option<ObjectId>) -> Result<ObjectId, CommandError> {
        let mut o = Object::empty();
        o.name = n.name.clone();
        o.visible = n.visible;
        o.transform = n.transform;
        o.material = n.material;
        o.body = n.body.clone();
        o.light = n.light;
        o.script = n.script.clone();
        o.parent = parent;
        if let Some(ms) = &n.mesh {
            let (mesh, lod) = self.resolve_mesh_state(ms)?;
            o.mesh = Some(mesh);
            o.lod = lod;
            o.local_bounds = self.assets.mesh_bounds(mesh);
        }
        let oid = self.scene.add(o);
        root.get_or_insert(oid);
        self.bind_node(oid, n.id.clone().expect("ids were filled"));
        for c in &n.children {
            self.build_node(c, Some(oid), root)?;
        }
        Ok(oid)
    }

    fn apply_delete(&mut self, node: &NodeId) -> Result<Applied, CommandError> {
        let oid = self.node_object(node)?;
        let parent = self.scene[oid].parent;
        let parent_id = parent.and_then(|p| self.node_id(p));
        let index = parent.and_then(|p| self.scene[p].children.iter().position(|c| *c == oid));
        let doc = self.capture(oid);
        let mut changed = Vec::new();
        collect_ids(&doc, &mut changed);
        self.scene.destroy(oid);
        Ok(applied(Command::Spawn { subtree: SceneDoc::new(doc), parent: parent_id, index }, changed))
    }

    /// Captures an object and its descendants exactly, giving every one of them a node id.
    pub fn capture(&mut self, oid: ObjectId) -> NodeDoc {
        // Pre-order by explicit stack (deep hierarchies must not overflow the stack).
        let mut order: Vec<(ObjectId, usize)> = Vec::new(); // (object, index of the parent entry)
        let mut stack = vec![(oid, usize::MAX)];
        while let Some((o, p)) = stack.pop() {
            order.push((o, p));
            let me = order.len() - 1;
            for c in self.scene[o].children.iter().rev() {
                stack.push((*c, me));
            }
        }
        let mut docs: Vec<Option<NodeDoc>> = Vec::with_capacity(order.len());
        for (o, _) in &order {
            let id = self.node_id(*o);
            let ob = &self.scene[*o];
            docs.push(Some(NodeDoc {
                id,
                name: ob.name.clone(),
                visible: ob.visible,
                transform: ob.transform,
                material: ob.material,
                body: ob.body.clone(),
                light: ob.light,
                mesh: self.mesh_state(ob),
                script: ob.script.clone(),
                children: Vec::new(),
            }));
        }
        // Children have larger indices than their parent: fold from the back; they arrive in reverse order.
        for i in (1..order.len()).rev() {
            let mut d = docs[i].take().expect("each entry is folded once");
            d.children.reverse();
            docs[order[i].1].as_mut().expect("the parent is folded later").children.push(d);
        }
        let mut root = docs[0].take().expect("the root exists");
        root.children.reverse();
        root
    }

    // ----- Properties and components --------------------------------------------------------------------

    fn apply_set(&mut self, node: &NodeId, path: &str, value: &Value) -> Result<Applied, CommandError> {
        let oid = self.node_object(node)?;
        let reg = registry();
        let (head, rest) = split_path(path);
        let inverse = if reg.component(head).is_some() {
            let before = self.component_state(oid, head).expect("registry components have a state");
            if before.is_absent() {
                return Err(CommandError::NoSuchComponent { node: node.clone(), component: head.to_string() });
            }
            // Rotations go through Euler angles (not bit-exact) and whole-component writes can drop runtime
            // handles: those are undone by restoring the captured state.
            let exact = head == "transform" || rest.is_empty();
            let old = if exact { None } else { reg.get_prop(&self.scene[oid], path).ok() };
            reg.set_prop(&mut self.scene[oid], path, value.clone())?;
            match old {
                Some(old) => Command::SetProperty { node: node.clone(), path: path.to_string(), value: old },
                None => Command::Restore { node: node.clone(), state: before },
            }
        } else {
            let old = reg.get_prop(&self.scene[oid], path)?;
            reg.set_prop(&mut self.scene[oid], path, value.clone())?;
            Command::SetProperty { node: node.clone(), path: path.to_string(), value: old }
        };
        Ok(applied(inverse, vec![node.clone()]))
    }

    fn apply_add(&mut self, node: &NodeId, component: &str, value: Option<&Value>) -> Result<Applied, CommandError> {
        let oid = self.node_object(node)?;
        let info = self.component_info(component)?;
        let before = self.component_state(oid, component).expect("registry components have a state");
        if component == "mesh" {
            let Some(v) = value else {
                return Err(CommandError::Reflect(ReflectError::ReadOnly { path: "mesh".into() }));
            };
            let source = MeshSource::from_value(v).map_err(|e| CommandError::Reflect(e.at("mesh")))?;
            self.apply_state(oid, ComponentState::Mesh(Some(MeshState { mesh: MeshDoc::from_source(source), lod: Vec::new() })))?;
        } else {
            (info.add)(&mut self.scene[oid], value).map_err(|e| CommandError::Reflect(e.at(component)))?;
        }
        let inverse = if before.is_absent() {
            Command::RemoveComponent { node: node.clone(), component: component.to_string() }
        } else {
            Command::Restore { node: node.clone(), state: before }
        };
        Ok(applied(inverse, vec![node.clone()]))
    }

    fn apply_remove(&mut self, node: &NodeId, component: &str) -> Result<Applied, CommandError> {
        let oid = self.node_object(node)?;
        let info = self.component_info(component)?;
        if info.required {
            return Err(CommandError::RequiredComponent { component: component.to_string() });
        }
        let before = self.component_state(oid, component).expect("registry components have a state");
        if before.is_absent() {
            return Err(CommandError::NoSuchComponent { node: node.clone(), component: component.to_string() });
        }
        (info.remove)(&mut self.scene[oid]);
        Ok(applied(Command::Restore { node: node.clone(), state: before }, vec![node.clone()]))
    }

    fn component_info(&self, name: &str) -> Result<&'static crate::registry::ComponentInfo, CommandError> {
        let reg = registry();
        reg.component(name).ok_or_else(|| CommandError::UnknownComponent {
            name: name.to_string(),
            suggestions: did_you_mean(name, reg.components.iter().map(|c| c.name)),
        })
    }

    pub(crate) fn mesh_state(&self, o: &Object) -> Option<MeshState> {
        let doc = |m: MeshId| MeshDoc { source: self.assets.mesh_source(m).clone(), handle: Some(m) };
        o.mesh.map(|m| MeshState { mesh: doc(m), lod: o.lod.iter().map(|&(d, m)| (d, doc(m))).collect() })
    }

    /// The exact state of a registry component of an object. `None` for an unknown name.
    pub fn component_state(&self, oid: ObjectId, name: &str) -> Option<ComponentState> {
        let o = self.scene.get(oid)?;
        Some(match name {
            "transform" => ComponentState::Transform(o.transform),
            "material" => ComponentState::Material(o.material),
            "body" => ComponentState::Body(o.body.clone()),
            "light" => ComponentState::Light(o.light),
            "mesh" => ComponentState::Mesh(self.mesh_state(o)),
            _ => return None,
        })
    }

    fn apply_state(&mut self, oid: ObjectId, state: ComponentState) -> Result<(), CommandError> {
        match state {
            ComponentState::Transform(t) => self.scene[oid].transform = t,
            ComponentState::Material(m) => self.scene[oid].material = m,
            ComponentState::Body(b) => self.scene[oid].body = b,
            ComponentState::Light(l) => self.scene[oid].light = l,
            ComponentState::Mesh(None) => {
                let o = &mut self.scene[oid];
                o.mesh = None;
                o.lod.clear();
                o.local_bounds = Aabb::EMPTY;
            }
            ComponentState::Mesh(Some(ms)) => {
                let (mesh, lod) = self.resolve_mesh_state(&ms)?;
                let bounds = self.assets.mesh_bounds(mesh);
                let o = &mut self.scene[oid];
                o.mesh = Some(mesh);
                o.lod = lod;
                o.local_bounds = bounds;
            }
        }
        Ok(())
    }

    fn resolve_mesh_state(&mut self, ms: &MeshState) -> Result<(MeshId, Vec<(f32, MeshId)>), CommandError> {
        let mesh = self.resolve_mesh(&ms.mesh)?;
        let mut lod = Vec::with_capacity(ms.lod.len());
        for (d, m) in &ms.lod {
            lod.push((*d, self.resolve_mesh(m)?));
        }
        Ok((mesh, lod))
    }

    /// The mesh a [`MeshDoc`] stands for: its handle when still valid, an existing mesh with the same recipe, or a
    /// new one built from the recipe.
    fn resolve_mesh(&mut self, d: &MeshDoc) -> Result<MeshId, CommandError> {
        if let Some(h) = d.handle {
            if self.assets.mesh(h).is_some() && self.assets.mesh_source(h) == &d.source {
                return Ok(h);
            }
        }
        if d.source == MeshSource::RuntimeOnly {
            return Err(CommandError::Invalid("a runtime-only mesh (made by code) cannot be rebuilt: it has no recipe".into()));
        }
        if let Some(i) = (0..self.assets.mesh_count()).find(|&i| self.assets.mesh_source(MeshId(i as u32)) == &d.source) {
            return Ok(MeshId(i as u32));
        }
        self.assets.add_mesh_from(&d.source).map_err(CommandError::Invalid)
    }

    // ----- Hierarchy -------------------------------------------------------------------------------------

    fn apply_reparent(&mut self, node: &NodeId, parent: Option<&NodeId>, index: Option<usize>) -> Result<Applied, CommandError> {
        let oid = self.node_object(node)?;
        let new_parent = parent.map(|p| self.node_object(p)).transpose()?;
        if let (Some(p), Some(pid)) = (new_parent, parent) {
            let mut cur = Some(p);
            while let Some(c) = cur {
                if c == oid {
                    return Err(CommandError::Cycle { node: node.clone(), parent: pid.clone() });
                }
                cur = self.scene.get(c).and_then(|o| o.parent);
            }
        }
        let old_parent = self.scene[oid].parent;
        let old_parent_id = old_parent.and_then(|p| self.node_id(p));
        let old_index = old_parent.and_then(|p| self.scene[p].children.iter().position(|c| *c == oid));
        self.scene.set_parent(oid, new_parent).map_err(CommandError::Invalid)?;
        if let (Some(p), Some(i)) = (new_parent, index) {
            self.place_child(p, oid, i);
        }
        let inverse = Command::Reparent { node: node.clone(), parent: old_parent_id, index: old_index };
        Ok(applied(inverse, vec![node.clone()]))
    }

    /// Moves `child` (already a child of `parent`) to `index` among the children (clamped).
    fn place_child(&mut self, parent: ObjectId, child: ObjectId, index: usize) {
        let kids = &mut self.scene[parent].children;
        if let Some(from) = kids.iter().position(|c| *c == child) {
            kids.remove(from);
            let at = index.min(kids.len());
            kids.insert(at, child);
        }
    }
}

fn applied(inverse: Command, changed: Vec<NodeId>) -> Applied {
    Applied { inverse, changed, resolved: None }
}

fn collect_ids(n: &NodeDoc, out: &mut Vec<NodeId>) {
    out.extend(n.id.iter().cloned());
    for c in &n.children {
        collect_ids(c, out);
    }
}

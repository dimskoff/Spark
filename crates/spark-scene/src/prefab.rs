//! Prefab instances (ADR 0003, rule 7): [`expand`] a scene so that every instance becomes the nodes of its prefab with
//! the overrides applied, and [`collapse`] a captured world back, writing only what differs from a fresh instance.
//!
//! In a file an instance is one node, `prefab = "prefabs/door.prefab"`:
//!
//! - the components written on the instance node replace the ones of the prefab's root (a whole component each);
//! - `[node.overrides]` changes the nodes of the prefab, keys `"<node id in the prefab>/<component>.<field>"`
//!   (`"leaf/material.color" = "#8a3b2f"`); `"<node>/<component>"` replaces or adds a whole component, `<component>` may be
//!   `node` for the `name` and `visible` of a prefab node;
//! - children added in the scene use `parent = "<instance id>"`, or `"<instance id>/<node id>"` to attach inside it.
//!
//! v1 has property overrides and added children only. A prefab with an instance inside it (nesting) is an error, and
//! so is anything that would remove or reparent a prefab node when saving: those are reported as warnings by
//! [`collapse`] and the prefab's version stays.
//!
//! There is no override bookkeeping: [`collapse`] diffs the live nodes against the prefab, so a value set back to the
//! prefab's value disappears from the file.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::path::PathBuf;

use spark_core::doc::{FileKind, FileNode, PrefabMember, RESERVED_COMPONENTS, SceneFile, mesh_from_value, normalize_component};
use spark_core::registry::registry;
use spark_core::{MeshSource, NodeId};
use spark_reflect::{FieldFlags, Value};

use crate::read::parse;

// ---------------------------------------------------------------------------------------------------------
// Override keys

/// `"leaf/material.color"` taken apart.
pub(crate) struct OverrideKey<'a> {
    pub node: &'a str,
    pub component: &'a str,
    pub field: Option<&'a str>,
}

/// What an override's component name is.
pub(crate) enum Target {
    /// `node`: the `name` and `visible` of a prefab node.
    Node,
    /// A registry component.
    Registry,
    /// `camera`, `script`: kept as plain maps, overridden whole.
    Reserved,
    Unknown,
}

pub(crate) fn split_override_key(k: &str) -> Result<OverrideKey<'_>, String> {
    let shape = "\"<node id in the prefab>/<component>.<field>\", e.g. \"leaf/material.color\"";
    let (node, rest) = k.split_once('/').ok_or_else(|| format!("the override key `{k}` must look like {shape}"))?;
    NodeId::new(node).map_err(|e| format!("the override key `{k}`: {e}"))?;
    let (component, field) = match rest.split_once('.') {
        Some((c, f)) => (c, Some(f)),
        None => (rest, None),
    };
    if component.is_empty() || field == Some("") {
        return Err(format!("the override key `{k}` must look like {shape}"));
    }
    if let Some(f) = field {
        if let Some((first, _)) = f.split_once('.') {
            return Err(format!("the override key `{k}`: override the whole field, write \"{node}/{component}.{first}\" = {{ ... }}"));
        }
    }
    Ok(OverrideKey { node, component, field })
}

pub(crate) fn override_target(k: &OverrideKey<'_>) -> Target {
    if k.component == "node" {
        Target::Node
    } else if RESERVED_COMPONENTS.contains(&k.component) {
        Target::Reserved
    } else if registry().component(k.component).is_some() {
        Target::Registry
    } else {
        Target::Unknown
    }
}

// ---------------------------------------------------------------------------------------------------------
// Errors and prefab files

/// A prefab could not be used. The message names the prefab file and, where there is one, the instance.
#[derive(Clone, Debug, PartialEq)]
pub struct PrefabError {
    /// The instance node of the scene (or the node of the prefab) the problem is about.
    pub node: Option<NodeId>,
    /// The prefab file as written in the scene.
    pub prefab: Option<String>,
    pub message: String,
}

impl PrefabError {
    fn new(node: Option<&NodeId>, prefab: Option<&str>, message: impl Into<String>) -> Self {
        Self { node: node.cloned(), prefab: prefab.map(str::to_string), message: message.into() }
    }
}

impl fmt::Display for PrefabError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(n) = &self.node {
            write!(f, "node '{n}': ")?;
        }
        if let Some(p) = &self.prefab {
            write!(f, "prefab '{p}': ")?;
        }
        f.write_str(&self.message)
    }
}

impl std::error::Error for PrefabError {}

/// Where prefab files come from. A closure `FnMut(&str) -> Result<SceneFile, String>` works; [`FsLoader`] reads the
/// game folder. Errors are plain text: [`expand`] adds the prefab's path and the instance to them.
pub trait PrefabSource {
    fn prefab(&mut self, path: &str) -> Result<SceneFile, String>;
}

impl<F: FnMut(&str) -> Result<SceneFile, String>> PrefabSource for F {
    fn prefab(&mut self, path: &str) -> Result<SceneFile, String> {
        self(path)
    }
}

/// Reads prefab files from a game folder (the pack of a shipped game or the disk) and keeps them parsed.
#[derive(Debug)]
pub struct FsLoader {
    root: PathBuf,
    confine: bool,
    cache: HashMap<String, SceneFile>,
}

impl FsLoader {
    /// `root` is the game folder; with `confine` a path must stay inside it (what game scripts get).
    pub fn new(root: impl Into<PathBuf>, confine: bool) -> Self {
        Self { root: root.into(), confine, cache: HashMap::new() }
    }

    /// The file a game path names.
    pub fn resolve(&self, path: &str) -> Result<PathBuf, String> {
        if self.confine {
            return spark_core::vfs::jail(&self.root, path);
        }
        let p = std::path::Path::new(path);
        Ok(if p.is_absolute() { p.to_path_buf() } else { self.root.join(p) })
    }

    /// Reads and parses a `.scene` or `.prefab` (not cached).
    pub fn read(&self, path: &str) -> Result<SceneFile, String> {
        let full = self.resolve(path)?;
        let text = spark_core::vfs::read_string(&full)?;
        parse(&text).map(|l| l.file).map_err(|e| format!("'{path}': {e}"))
    }
}

impl PrefabSource for FsLoader {
    fn prefab(&mut self, path: &str) -> Result<SceneFile, String> {
        if let Some(f) = self.cache.get(path) {
            return Ok(f.clone());
        }
        let f = self.read(path)?;
        self.cache.insert(path.to_string(), f.clone());
        Ok(f)
    }
}

/// A checked prefab: the file and the index of its root.
struct Checked {
    file: SceneFile,
    root: usize,
}

fn fetch(source: &mut dyn PrefabSource, path: &str, instance: &NodeId) -> Result<Checked, PrefabError> {
    let err = |m: String| PrefabError::new(Some(instance), Some(path), m);
    let file = source.prefab(path).map_err(err)?;
    if file.kind != FileKind::Prefab {
        return Err(err("this is a scene ([scene] header), not a prefab: a prefab file has a [prefab] header".into()));
    }
    let roots: Vec<usize> = file.nodes.iter().enumerate().filter(|(_, n)| n.parent.is_none()).map(|(i, _)| i).collect();
    let [root] = roots[..] else {
        return Err(err(format!("a prefab has exactly one root node (a [[node]] without `parent`), this one has {}", roots.len())));
    };
    for n in &file.nodes {
        if let Some(inner) = &n.prefab {
            return Err(err(format!(
                "node '{}' is an instance of '{inner}': nested prefabs are not supported yet (prefab format v1 has property overrides and added children only)",
                n.id
            )));
        }
        if !n.overrides.is_empty() || n.parent_in.is_some() {
            return Err(err(format!(
                "node '{}': `overrides` and `parent = \"a/b\"` belong to prefab instances, which a prefab cannot contain yet",
                n.id
            )));
        }
    }
    Ok(Checked { file, root })
}

fn node_list(file: &SceneFile) -> String {
    file.nodes.iter().map(|n| n.id.as_str()).collect::<Vec<_>>().join(", ")
}

// ---------------------------------------------------------------------------------------------------------
// expand

struct InstanceIds {
    path: String,
    /// Prefab node id -> id in the expanded scene.
    ids: HashMap<NodeId, NodeId>,
}

/// FNV-1a of `instance/node#salt` as 8 hex digits, probed until it is free: the ids of a prefab's nodes in a scene
/// are the same on every load, and never clash with the ids of the file or (`taken`) of the world.
fn derive_id(instance: &NodeId, node: &NodeId, used: &mut HashSet<NodeId>, taken: &dyn Fn(&NodeId) -> bool) -> NodeId {
    let mut salt = 0u32;
    loop {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in format!("{instance}/{node}#{salt}").bytes() {
            h = (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3);
        }
        let id = NodeId::new(&format!("{:08x}", (h >> 32) as u32)).expect("8 hex digits are a valid id");
        if !used.contains(&id) && !taken(&id) {
            used.insert(id.clone());
            return id;
        }
        salt += 1;
    }
}

/// Turns every prefab instance of `file` into the nodes of its prefab with the overrides applied.
///
/// The instance root keeps the id of the instance node; the other nodes get derived ids (unique in the file and not
/// `taken`). Every expanded node carries a [`PrefabMember`], which is what [`spark_core::doc::instantiate`] uses to
/// remember the instance. Nodes that are not instances are copied; a node whose `parent` is `"door1/leaf"` is attached
/// to that node of the instance. A file without instances comes back unchanged.
pub fn expand(file: &SceneFile, source: &mut dyn PrefabSource, taken: &dyn Fn(&NodeId) -> bool) -> Result<SceneFile, PrefabError> {
    let is_instance = |n: &FileNode| n.prefab.is_some() && n.member.is_none();
    if !file.nodes.iter().any(is_instance) {
        return Ok(file.clone());
    }
    let mut used: HashSet<NodeId> = file.nodes.iter().map(|n| n.id.clone()).collect();
    let mut prefabs: HashMap<String, Checked> = HashMap::new();
    let mut instances: HashMap<NodeId, InstanceIds> = HashMap::new();
    for n in file.nodes.iter().filter(|n| is_instance(n)) {
        let path = n.prefab.as_deref().expect("an instance has a prefab");
        if !prefabs.contains_key(path) {
            prefabs.insert(path.to_string(), fetch(source, path, &n.id)?);
        }
        let pf = &prefabs[path];
        let mut ids = HashMap::new();
        for (i, pn) in pf.file.nodes.iter().enumerate() {
            let id = if i == pf.root { n.id.clone() } else { derive_id(&n.id, &pn.id, &mut used, taken) };
            ids.insert(pn.id.clone(), id);
        }
        instances.insert(n.id.clone(), InstanceIds { path: path.to_string(), ids });
    }

    let mut out = file.clone();
    out.nodes.clear();
    for n in &file.nodes {
        let parent = resolve_parent(n, &instances, &prefabs)?;
        if !is_instance(n) {
            let mut c = n.clone();
            c.parent = parent;
            c.parent_in = None;
            out.nodes.push(c);
            continue;
        }
        let path = n.prefab.as_deref().expect("an instance has a prefab");
        let pf = &prefabs[path];
        let ids = &instances[&n.id].ids;
        let mut group: Vec<FileNode> = Vec::with_capacity(pf.file.nodes.len());
        for (i, pn) in pf.file.nodes.iter().enumerate() {
            let mut m = pn.clone();
            m.id = ids[&pn.id].clone();
            m.member = Some(PrefabMember { instance: n.id.clone(), node: pn.id.clone() });
            if i == pf.root {
                m.parent = parent.clone();
                if !n.name.is_empty() {
                    m.name = n.name.clone();
                }
                m.visible = pn.visible && n.visible;
                if !n.tags.is_empty() {
                    m.tags = n.tags.clone();
                }
                for (name, v) in &n.components {
                    m.set_component(name, v.clone());
                }
                m.prefab = Some(path.to_string());
                group.insert(0, m);
            } else {
                m.parent = pn.parent.as_ref().map(|p| ids[p].clone());
                group.push(m);
            }
        }
        for (k, v) in &n.overrides {
            let fail = |m: String| PrefabError::new(Some(&n.id), Some(path), format!("override `{k}`: {m}"));
            let key = split_override_key(k).map_err(|m| PrefabError::new(Some(&n.id), Some(path), m))?;
            let target = group
                .iter_mut()
                .find(|g| g.member.as_ref().is_some_and(|m| m.node.as_str() == key.node))
                .ok_or_else(|| fail(format!("the prefab has no node '{}' (its nodes: {})", key.node, node_list(&pf.file))))?;
            apply_override(target, &key, v).map_err(fail)?;
        }
        out.nodes.extend(group);
    }
    Ok(out)
}

/// The parent of `n` in the expanded scene (`parent` and `parent_in` resolved).
fn resolve_parent(
    n: &FileNode,
    instances: &HashMap<NodeId, InstanceIds>,
    prefabs: &HashMap<String, Checked>,
) -> Result<Option<NodeId>, PrefabError> {
    let (Some(p), Some(inner)) = (&n.parent, &n.parent_in) else { return Ok(n.parent.clone()) };
    let Some(inst) = instances.get(p) else {
        return Err(PrefabError::new(
            Some(&n.id),
            None,
            format!("`parent = \"{p}/{inner}\"` needs '{p}' to be a prefab instance (a node with `prefab = \"...\"`)"),
        ));
    };
    inst.ids.get(inner).cloned().map(Some).ok_or_else(|| {
        PrefabError::new(
            Some(&n.id),
            Some(&inst.path),
            format!("`parent = \"{p}/{inner}\"`: the prefab has no node '{inner}' (its nodes: {})", node_list(&prefabs[&inst.path].file)),
        )
    })
}

/// Applies one override to the expanded node it names.
fn apply_override(node: &mut FileNode, k: &OverrideKey<'_>, v: &Value) -> Result<(), String> {
    match (override_target(k), k.field) {
        (Target::Unknown, _) => Err(format!("'{}' is not a component", k.component)),
        (Target::Node, Some("name")) => match v {
            Value::Str(s) => {
                node.name = s.clone();
                Ok(())
            }
            other => Err(format!("`name` must be a string, found {}", other.type_name())),
        },
        (Target::Node, Some("visible")) => match v {
            Value::Bool(b) => {
                node.visible = *b;
                Ok(())
            }
            other => Err(format!("`visible` must be true or false, found {}", other.type_name())),
        },
        (Target::Node, Some("tags")) => match v {
            Value::List(items) => {
                node.tags = items.iter().filter_map(|t| if let Value::Str(s) = t { Some(s.clone()) } else { None }).collect();
                Ok(())
            }
            other => Err(format!("`tags` must be a list of strings, found {}", other.type_name())),
        },
        (Target::Node, _) => Err("`node` has the fields `name`, `visible` and `tags`".into()),
        (Target::Reserved, None) => {
            node.set_component(k.component, v.clone());
            Ok(())
        }
        (Target::Reserved, Some(_)) => Err(format!("`{}` is overridden whole", k.component)),
        (Target::Registry, None) => {
            let v = normalize_component(k.component, v).map_err(|e| e.to_string())?;
            node.set_component(k.component, v);
            Ok(())
        }
        (Target::Registry, Some(_)) if k.component == "mesh" => Err("a mesh is overridden whole".into()),
        (Target::Registry, Some(field)) => {
            let mut cur = match node.component(k.component) {
                Some(c) => c.clone(),
                None => normalize_component(k.component, &Value::Map(Vec::new())).map_err(|e| e.to_string())?,
            };
            let Value::Map(pairs) = &mut cur else { return Err(format!("component `{}` is not a table", k.component)) };
            match pairs.iter_mut().find(|(n, _)| n == field) {
                Some(slot) => slot.1 = v.clone(),
                None => pairs.push((field.to_string(), v.clone())),
            }
            // Validates the value (ranges, types) and widens numbers the way every component value is.
            let norm = normalize_component(k.component, &cur).map_err(|e| e.to_string())?;
            node.set_component(k.component, norm);
            Ok(())
        }
    }
}

// ---------------------------------------------------------------------------------------------------------
// collapse

/// What [`collapse`] made.
#[derive(Clone, Debug, PartialEq)]
pub struct Collapsed {
    /// The document to write: instances are one node again, with the components and overrides that differ from the
    /// prefab; nodes added to an instance are normal nodes that name their parent.
    pub file: SceneFile,
    /// Changes to prefab instances that a v1 file cannot hold (a removed or reparented prefab node, a removed component,
    /// a mesh made by code). The prefab's version stays; nothing is dropped without a line here.
    pub warnings: Vec<String>,
}

/// The inverse of [`expand`] for a captured world ([`spark_core::doc::capture`]): the nodes of every live instance are
/// compared with their prefab and replaced by one instance node with the differences.
pub fn collapse(captured: &SceneFile, source: &mut dyn PrefabSource) -> Result<Collapsed, PrefabError> {
    let mut warnings = Vec::new();
    let is_root = |n: &FileNode| n.prefab.is_some() && n.member.as_ref().is_some_and(|m| m.instance == n.id);
    if !captured.nodes.iter().any(|n| n.member.is_some()) {
        return Ok(Collapsed { file: captured.clone(), warnings });
    }
    // Captured node id -> (instance root, node in the prefab), and back.
    let mut member_of: HashMap<&NodeId, &PrefabMember> = HashMap::new();
    let mut by_prefab: HashMap<(&NodeId, &NodeId), &FileNode> = HashMap::new();
    for n in &captured.nodes {
        if let Some(m) = &n.member {
            member_of.insert(&n.id, m);
            by_prefab.insert((&m.instance, &m.node), n);
        }
    }
    let mut prefabs: HashMap<String, Checked> = HashMap::new();
    let mut out = captured.clone();
    out.nodes.clear();
    for n in &captured.nodes {
        match &n.member {
            Some(m) if !is_root(n) => {
                // A prefab node below the root: part of its instance node. Its parent must still be the prefab's.
                let Some(pf) = captured.node(&m.instance).and_then(|r| r.prefab.as_deref()).and_then(|p| prefabs.get(p)) else {
                    continue;
                };
                let expected =
                    pf.file.node(&m.node).and_then(|pn| pn.parent.as_ref()).and_then(|pp| by_prefab.get(&(&m.instance, pp))).map(|c| &c.id);
                if n.parent.as_ref() != expected {
                    warnings.push(format!(
                        "node '{}' ('{}' of instance '{}'): it was moved out of its place in the prefab; reparenting a prefab's nodes is not saved (prefab format v1)",
                        n.id, m.node, m.instance
                    ));
                }
                continue;
            }
            _ => {}
        }
        let parent = collapsed_parent(n, &member_of);
        let mut c = n.clone();
        c.parent = parent.0;
        c.parent_in = parent.1;
        c.member = None;
        if !is_root(n) {
            out.nodes.push(c);
            continue;
        }
        let path = n.prefab.as_deref().expect("a root has a prefab").to_string();
        if !prefabs.contains_key(&path) {
            prefabs.insert(path.clone(), fetch(source, &path, &n.id)?);
        }
        let pf = &prefabs[&path];
        let mut inst = FileNode::new(n.id.clone());
        inst.components.clear();
        inst.parent = c.parent;
        inst.parent_in = c.parent_in;
        inst.prefab = Some(path.clone());
        let mut w = |msg: String| warnings.push(format!("instance '{}' of '{path}': {msg}", n.id));
        for (i, pn) in pf.file.nodes.iter().enumerate() {
            let Some(live) = by_prefab.get(&(&n.id, &pn.id)) else {
                w(format!(
                    "the prefab's node '{}' was removed; removing prefab nodes is not saved (prefab format v1), it comes back on load",
                    pn.id
                ));
                continue;
            };
            let at = if i == pf.root { String::new() } else { format!("{}/", pn.id) };
            diff_node(live, pn, i == pf.root, &at, &mut inst, &mut w);
        }
        out.nodes.push(inst);
    }
    Ok(Collapsed { file: out, warnings })
}

/// `(parent, parent_in)` of a node that is not part of an instance: a node added under a prefab node attaches to the
/// instance (`"door1"` for its root, `"door1/leaf"` for another node).
fn collapsed_parent(n: &FileNode, member_of: &HashMap<&NodeId, &PrefabMember>) -> (Option<NodeId>, Option<NodeId>) {
    let Some(p) = &n.parent else { return (None, None) };
    match member_of.get(p) {
        Some(m) if m.instance != *p => (Some(m.instance.clone()), Some(m.node.clone())),
        _ => (Some(p.clone()), None),
    }
}

/// A component value without what a file cannot hold (runtime handles), or `None` when nothing of it can be saved.
fn saveable(name: &str, v: &Value, warn: &mut dyn FnMut(String)) -> Option<Value> {
    if name == "mesh" {
        return match mesh_from_value(v) {
            Ok((MeshSource::RuntimeOnly, _)) => {
                warn("the mesh was made by code (Mesh.custom) and has no recipe; the prefab's mesh is kept".into());
                None
            }
            Ok((_, lod)) if lod.iter().any(|(_, s)| *s == MeshSource::RuntimeOnly) => {
                warn("a level of detail was made by code and has no recipe; the prefab's mesh is kept".into());
                None
            }
            _ => Some(v.clone()),
        };
    }
    let (Some(info), Value::Map(pairs)) = (registry().component(name), v) else { return Some(v.clone()) };
    let ty = info.info();
    let mut kept = Vec::with_capacity(pairs.len());
    for (k, val) in pairs {
        let handle = ty.field(k).is_some_and(|f| f.flags.contains(FieldFlags::TRANSIENT)) && matches!(val, Value::Asset(_));
        if handle {
            warn(format!("{name}.{k} is a runtime handle (a texture or shader) and cannot be saved; the prefab's value is kept"));
        } else {
            kept.push((k.clone(), val.clone()));
        }
    }
    Some(Value::Map(kept))
}

/// Compares the live node with the prefab's and records the differences on `inst` (the root: whole components, name and
/// visibility; any other node: overrides keyed `"<at><component>.<field>"`).
fn diff_node(live: &FileNode, prefab: &FileNode, root: bool, at: &str, inst: &mut FileNode, warn: &mut dyn FnMut(String)) {
    let describe = |what: String| if root { what } else { format!("node '{}': {what}", prefab.id) };
    if live.name != prefab.name && !live.name.is_empty() {
        if root {
            inst.name = live.name.clone();
        } else {
            inst.overrides.push((format!("{at}node.name"), Value::Str(live.name.clone())));
        }
    }
    if live.visible != prefab.visible {
        if root && !live.visible {
            inst.visible = false;
        } else if root {
            warn(describe("the prefab's root is hidden and the instance is shown; that cannot be saved (hide it in the prefab)".into()));
        } else {
            inst.overrides.push((format!("{at}node.visible"), Value::Bool(live.visible)));
        }
    }
    if live.tags != prefab.tags {
        if root && !live.tags.is_empty() {
            inst.tags = live.tags.clone();
        } else if root {
            warn(describe("the prefab's root has tags and the instance has none; that cannot be saved (remove them in the prefab)".into()));
        } else {
            inst.overrides.push((format!("{at}node.tags"), Value::List(live.tags.iter().map(|t| Value::Str(t.clone())).collect())));
        }
    }
    for (name, cv) in &live.components {
        let mut w = |m: String| warn(describe(format!("{name}: {m}")));
        let Some(cv) = saveable(name, cv, &mut w) else { continue };
        match prefab.component(name) {
            Some(pv) if *pv == cv => {}
            Some(_) if root => inst.set_component(name, cv),
            None if root => inst.set_component(name, cv),
            None => inst.overrides.push((format!("{at}{name}"), cv)),
            Some(pv) => match (&cv, pv) {
                (Value::Map(cur), Value::Map(base)) if name != "mesh" => {
                    for (k, val) in cur {
                        if base.iter().find(|(bk, _)| bk == k).map(|(_, b)| b) != Some(val) {
                            inst.overrides.push((format!("{at}{name}.{k}"), val.clone()));
                        }
                    }
                }
                _ => inst.overrides.push((format!("{at}{name}"), cv)),
            },
        }
    }
    for (name, _) in &prefab.components {
        if live.component(name).is_none() {
            warn(describe(format!("the component '{name}' was removed; removing a prefab's components is not saved (prefab format v1)")));
        }
    }
}

//! Stable node ids (ADR 0003, ADR 0005): the name commands, scene files and devtools use for an object.
//!
//! An id is a string of 1 to 32 characters of `[a-z0-9_-]`; generated ones are 8 lowercase hex digits. Unlike
//! [`ObjectId`] (slot + generation) it survives a delete followed by an undo, and it is what a saved scene stores.
//!
//! Ids are **lazy**: an object made by a script has none until something needs one (a command, a dump, a save),
//! see [`World::node_id`]. Once assigned it stays with the object for the session. The lookup table
//! (`NodeId -> ObjectId`) lives in [`World`]; entries of destroyed objects are not removed eagerly, every lookup
//! validates against the scene.

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use crate::scene::ObjectId;
use crate::world::World;

/// Stable string id of a node. Cheap to clone.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeId(Arc<str>);

/// The text is not a valid node id.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InvalidNodeId(pub String);

impl fmt::Display for InvalidNodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid node id {:?}: use 1 to {} characters of a-z, 0-9, '_' or '-'", self.0, NodeId::MAX_LEN)
    }
}

impl std::error::Error for InvalidNodeId {}

impl NodeId {
    /// Longest accepted id.
    pub const MAX_LEN: usize = 32;

    /// Validates `s` (`[a-z0-9_-]{1,32}`).
    pub fn new(s: &str) -> Result<Self, InvalidNodeId> {
        let ok = !s.is_empty()
            && s.len() <= Self::MAX_LEN
            && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-');
        if ok { Ok(Self(Arc::from(s))) } else { Err(InvalidNodeId(s.to_string())) }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for NodeId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

/// `NodeId -> ObjectId` table plus the id generator. Owned by [`World`].
#[derive(Debug)]
pub(crate) struct NodeIndex {
    map: HashMap<NodeId, ObjectId>,
    /// splitmix64 state. Fixed seed: generated ids are deterministic for a given sequence of requests, and a new id
    /// never equals a live one (checked on generation).
    state: u64,
}

impl Default for NodeIndex {
    fn default() -> Self {
        Self { map: HashMap::new(), state: 0x5EED_5A9B_C0DE_1234 }
    }
}

impl NodeIndex {
    pub(crate) fn clear(&mut self) {
        self.map.clear();
    }

    fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
}

impl World {
    /// The live object with this node id.
    pub fn find_node(&self, id: &NodeId) -> Option<ObjectId> {
        let oid = *self.nodes.map.get(id)?;
        (self.scene.get(oid)?.node_id.as_ref() == Some(id)).then_some(oid)
    }

    /// The node id of an object, assigning a fresh one the first time. `None` when the object does not exist.
    pub fn node_id(&mut self, oid: ObjectId) -> Option<NodeId> {
        if let Some(id) = self.scene.get(oid)?.node_id.clone() {
            return Some(id);
        }
        let id = self.fresh_node_id(&|_| false);
        self.bind_node(oid, id.clone());
        Some(id)
    }

    /// The node id of an object if it already has one (never assigns).
    pub fn node_id_of(&self, oid: ObjectId) -> Option<&NodeId> {
        self.scene.get(oid)?.node_id.as_ref()
    }

    /// A random-looking 8 hex digit id that no live node and no id for which `taken` is true uses.
    pub(crate) fn fresh_node_id(&mut self, taken: &dyn Fn(&NodeId) -> bool) -> NodeId {
        loop {
            let id = NodeId(Arc::from(format!("{:08x}", (self.nodes.next_u64() >> 32) as u32)));
            if self.find_node(&id).is_none() && !taken(&id) {
                return id;
            }
        }
    }

    /// Gives `oid` the id (the caller checked it is free).
    pub(crate) fn bind_node(&mut self, oid: ObjectId, id: NodeId) {
        let len = self.nodes.map.len();
        if len > 64 && len > 2 * self.scene.len() {
            let scene = &self.scene;
            self.nodes.map.retain(|k, v| scene.get(*v).and_then(|o| o.node_id.as_ref()) == Some(k));
        }
        if let Some(o) = self.scene.get_mut(oid) {
            o.node_id = Some(id.clone());
            self.nodes.map.insert(id, oid);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::Object;

    #[test]
    fn ids_are_validated() {
        assert!(NodeId::new("a1f3c09e").is_ok());
        assert!(NodeId::new("door_leaf-2").is_ok());
        for bad in ["", "Upper", "has space", "a/b", &"x".repeat(33)] {
            assert!(NodeId::new(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn ids_are_lazy_stable_and_unique() {
        let mut w = World::new();
        let a = w.scene.add(Object::empty());
        let b = w.scene.add(Object::empty());
        assert!(w.node_id_of(a).is_none());
        let ia = w.node_id(a).unwrap();
        assert_eq!(w.node_id(a), Some(ia.clone()));
        let ib = w.node_id(b).unwrap();
        assert_ne!(ia, ib);
        assert_eq!(ia.as_str().len(), 8);
        assert_eq!(w.find_node(&ia), Some(a));
        w.scene.destroy(a);
        assert_eq!(w.find_node(&ia), None, "stale entries are validated");
        assert_eq!(w.node_id(a), None);
    }
}

//! The component registry over [`Object`] (ADR 0004, ADR 0005).
//!
//! An object is a node (name, visibility, hierarchy) plus a closed set of components: `transform`, `mesh`,
//! `material`, `body` and `light` (`camera`, `script`, `tags` come later). [`registry()`] lists them with their field
//! metadata ([`TypeInfo`]) and four accessors each, so that scripts, the inspector, scene files and commands all read
//! and write an object the same way, with the same validation:
//!
//! ```
//! use spark_core::{Object, registry::registry};
//! use spark_reflect::Value;
//!
//! let mut o = Object::empty();
//! o.set_prop("transform.position.y", Value::Float(2.0)).unwrap();
//! assert_eq!(o.get_prop("transform.position.y"), Ok(Value::Float(2.0)));
//! let body = registry().component("body").unwrap();
//! (body.add)(&mut o, None).unwrap(); // a default dynamic body
//! o.set_prop("body.mass", Value::Float(80.0)).unwrap();
//! assert!(o.set_prop("body.mass", Value::Float(0.0)).is_err()); // masses must be greater than 0
//! ```
//!
//! The renderer, the physics sync and the scripting hot paths keep reading the typed fields of `Object` directly;
//! the registry is the dynamic door for everything else. The accessors copy the component (small `Copy`/`Clone`
//! structs), so they are not meant for per-frame work on thousands of objects.
//!
//! Adding a field to `Object` needs a decision here: see [`OBJECT_FIELDS`] and the exhaustive test next to it.

use spark_reflect::{Reflect, ReflectError, TypeInfo, Value, did_you_mean, split_path};

use crate::assets::MeshId;
use crate::material::Material;
use crate::mesh::Aabb;
use crate::physics::Body;
use crate::scene::{Light, Object, Transform};

/// Which Luau object the `lua` names of a component's fields belong to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LuaScope {
    /// Flat names on the object: `obj.position`, `obj.color`, `obj.mass`.
    Object,
    /// Members of a handle returned by the object: `obj.light.intensity` (flat `obj.color` is the material's).
    Handle(&'static str),
}

/// A component of an object and how to reach it.
#[derive(Clone, Copy, Debug)]
pub struct ComponentInfo {
    /// Registry (and file) name: `"body"`.
    pub name: &'static str,
    /// Field metadata (`TypeInfo` of the component struct). A function because a `static` initializer cannot call
    /// `Reflect::type_info()`; see [`ComponentInfo::info`].
    pub type_info: fn() -> &'static TypeInfo,
    /// Always present (`transform`, `material`): cannot be removed.
    pub required: bool,
    /// What the `lua` names of the fields refer to.
    pub lua: LuaScope,
    /// The whole component as a value; `None` = the object has no such component.
    pub get: fn(&Object) -> Option<Value>,
    /// Reads a path below the component (`"shape.radius"`, `"position.x"`; empty = the whole value).
    pub get_path: fn(&Object, &str) -> Result<Value, ReflectError>,
    /// Writes a path below the component with validation (empty path = replace the whole component).
    /// Errors when the object has no such component.
    pub set: fn(&mut Object, &str, Value) -> Result<(), ReflectError>,
    /// Adds the component with the given value, or with defaults for `None`. An existing component is replaced
    /// (like `obj:add_body()`). Fails for components that cannot be created from a value yet (`mesh`).
    pub add: fn(&mut Object, Option<&Value>) -> Result<(), ReflectError>,
    /// Removes the component. `false` when it was not there or is `required`.
    pub remove: fn(&mut Object) -> bool,
}

impl ComponentInfo {
    pub fn info(&self) -> &'static TypeInfo {
        (self.type_info)()
    }
}

/// All components, plus the node-level fields (`name`, `visible`).
#[derive(Debug)]
pub struct Registry {
    /// `TypeInfo` of the node fields: `name`, `visible`.
    pub node: fn() -> &'static TypeInfo,
    pub components: &'static [ComponentInfo],
}

/// The registry of components of [`Object`].
pub fn registry() -> &'static Registry {
    static REGISTRY: Registry = Registry {
        node: NodeProps::type_info,
        components: &[component::<Transform>(), component::<MeshRef>(), component::<Material>(), component::<Body>(), component::<Light>()],
    };
    &REGISTRY
}

impl Registry {
    pub fn component(&self, name: &str) -> Option<&'static ComponentInfo> {
        self.components.iter().find(|c| c.name == name)
    }

    pub fn node_info(&self) -> &'static TypeInfo {
        (self.node)()
    }

    /// Names a path can start with: the node fields and the components.
    fn head_names(&self) -> impl Iterator<Item = &'static str> {
        self.node_info().field_names().chain(self.components.iter().map(|c| c.name))
    }

    fn unknown(&self, head: &str) -> ReflectError {
        ReflectError::UnknownField { ty: "Object", path: head.to_string(), suggestions: did_you_mean(head, self.head_names()) }
    }

    /// `"material.color"`, `"body.mass"`, `"transform.position.x"`, `"name"`, `"visible"`. The empty path is the whole object.
    pub fn get_prop(&self, o: &Object, path: &str) -> Result<Value, ReflectError> {
        if path.is_empty() {
            return Ok(self.snapshot(o));
        }
        let (head, rest) = split_path(path);
        if self.node_info().field(head).is_some() {
            return NodeProps::of(o).get(path);
        }
        let c = self.component(head).ok_or_else(|| self.unknown(head))?;
        if rest.is_empty() {
            return Ok((c.get)(o).unwrap_or(Value::None));
        }
        (c.get_path)(o, rest).map_err(|e| e.at(head))
    }

    /// Writes `path` (see [`Registry::get_prop`]) with validation: type, range, finite, read-only.
    pub fn set_prop(&self, o: &mut Object, path: &str, v: Value) -> Result<(), ReflectError> {
        let (head, rest) = split_path(path);
        if self.node_info().field(head).is_some() {
            let mut node = NodeProps::of(o);
            node.set(path, v)?;
            node.apply(o);
            return Ok(());
        }
        let c = self.component(head).ok_or_else(|| self.unknown(head))?;
        (c.set)(o, rest, v).map_err(|e| e.at(head))
    }

    /// Node fields and every present component, as one map.
    fn snapshot(&self, o: &Object) -> Value {
        let mut pairs = Vec::new();
        if let Value::Map(node) = NodeProps::of(o).to_value() {
            pairs.extend(node);
        }
        for c in self.components {
            if let Some(v) = (c.get)(o) {
                pairs.push((c.name.to_string(), v));
            }
        }
        Value::Map(pairs)
    }
}

impl Object {
    /// Reads `"material.color"`, `"body.mass"`, `"transform.position.x"`, `"name"`, ... through the registry.
    pub fn get_prop(&self, path: &str) -> Result<Value, ReflectError> {
        registry().get_prop(self, path)
    }

    /// Writes a property through the registry (validated). Errors name the path and, for unknown names, suggest the
    /// closest ones.
    pub fn set_prop(&mut self, path: &str, value: Value) -> Result<(), ReflectError> {
        registry().set_prop(self, path, value)
    }
}

// ---------------------------------------------------------------------------------------------------------
// What every field of `Object` is

/// What the registry does with a field of [`Object`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldDecision {
    /// Node level (`name`, `visible`): addressed as `"name"` / `"visible"`.
    Node,
    /// Part of the named component.
    Component(&'static str),
    /// The hierarchy (`parent`, `children`): changed with `Scene::set_parent`, later by the `Reparent` command.
    Hierarchy,
    /// A cache derived from other fields (`local_bounds` from the mesh): never stored or edited.
    Derived,
    /// Runtime state of the engine (`interp`): not data of the document.
    Runtime,
}

/// A decision for every field of [`Object`]. Adding a field to `Object` fails the test
/// `every_object_field_has_a_registry_decision` until the field is listed here.
pub const OBJECT_FIELDS: &[(&str, FieldDecision)] = &[
    ("name", FieldDecision::Node),
    ("transform", FieldDecision::Component("transform")),
    ("mesh", FieldDecision::Component("mesh")),
    ("lod", FieldDecision::Component("mesh")),
    ("material", FieldDecision::Component("material")),
    ("visible", FieldDecision::Node),
    // The stable id is not a property (never set through a path); `crate::node_id` owns it.
    ("node_id", FieldDecision::Runtime),
    ("body", FieldDecision::Component("body")),
    ("light", FieldDecision::Component("light")),
    ("parent", FieldDecision::Hierarchy),
    ("children", FieldDecision::Hierarchy),
    ("local_bounds", FieldDecision::Derived),
    ("interp", FieldDecision::Runtime),
];

// ---------------------------------------------------------------------------------------------------------
// Node fields as a reflected view

/// The node-level fields of an object, as a reflected struct (a copy: `name` and `visible`).
#[derive(Clone, Debug, PartialEq, Reflect)]
pub struct NodeProps {
    /// Name shown in the hierarchy and used by `find(name)`.
    #[reflect(lua = "name")]
    pub name: String,
    /// Drawn (children inherit it being off).
    #[reflect(lua = "visible")]
    pub visible: bool,
}

impl Default for NodeProps {
    fn default() -> Self {
        Self { name: String::new(), visible: true }
    }
}

impl NodeProps {
    fn of(o: &Object) -> Self {
        Self { name: o.name.clone(), visible: o.visible }
    }

    fn apply(self, o: &mut Object) {
        o.name = self.name;
        o.visible = self.visible;
    }
}

// ---------------------------------------------------------------------------------------------------------
// The `mesh` component: a view over `Object::mesh` + `Object::lod`

/// One level of detail: from `distance` meters away the object draws `mesh` instead.
#[derive(Clone, Copy, Debug, PartialEq, Reflect)]
pub struct LodLevel {
    /// Distance from the camera, in meters, from which this mesh is drawn.
    #[reflect(range = 0.0..=f32::MAX, unit = "m", default = 0.0)]
    pub distance: f32,
    /// The mesh drawn (a runtime handle).
    #[reflect(readonly, transient, default = MeshId(0))]
    pub mesh: MeshId,
}

/// The mesh an object draws and its level-of-detail meshes (`Object::mesh`, `Object::lod`). Present when the object
/// has a mesh. Read-only for now: meshes are runtime handles until mesh recipes exist (step C-11).
#[derive(Clone, Debug, PartialEq, Reflect)]
pub struct MeshRef {
    /// The mesh drawn (a runtime handle).
    #[reflect(readonly, transient, lua = "mesh", default = MeshId(0))]
    pub mesh: MeshId,
    /// Level of detail: from each distance on the renderer draws that mesh instead of `mesh`.
    #[reflect(readonly, transient, lua = "lod", default = Vec::new())]
    pub lod: Vec<LodLevel>,
}

// ---------------------------------------------------------------------------------------------------------
// Accessors

/// A component stored in an [`Object`]. The accessors in [`ComponentInfo`] are the generic functions below.
trait Component: Reflect + Clone {
    const NAME: &'static str;
    const REQUIRED: bool;
    const LUA: LuaScope = LuaScope::Object;
    /// `false`: every write is refused (`mesh`).
    const WRITABLE: bool = true;

    fn load(o: &Object) -> Option<Self>;
    fn store(self, o: &mut Object);
    fn clear(o: &mut Object) -> bool;
    /// A new component with default values; `None` = cannot be created without a value.
    fn create() -> Option<Self>;
    /// A component from `add`.
    fn build(v: Option<&Value>) -> Result<Self, ReflectError> {
        match v {
            Some(v) => Self::from_value(v),
            None => Self::create().ok_or(ReflectError::ReadOnly { path: String::new() }),
        }
    }
}

fn absent() -> ReflectError {
    ReflectError::WrongType { path: String::new(), expected: "component", got: "none" }
}

fn c_get<C: Component>(o: &Object) -> Option<Value> {
    C::load(o).map(|c| c.to_value())
}

fn c_get_path<C: Component>(o: &Object, path: &str) -> Result<Value, ReflectError> {
    C::load(o).ok_or_else(absent)?.get(path)
}

fn c_set<C: Component>(o: &mut Object, path: &str, v: Value) -> Result<(), ReflectError> {
    if !C::WRITABLE {
        return Err(ReflectError::ReadOnly { path: path.to_string() });
    }
    let mut c = C::load(o).ok_or_else(absent)?;
    c.set(path, v)?;
    c.store(o);
    Ok(())
}

fn c_add<C: Component>(o: &mut Object, v: Option<&Value>) -> Result<(), ReflectError> {
    if !C::WRITABLE {
        return Err(ReflectError::ReadOnly { path: String::new() });
    }
    C::build(v)?.store(o);
    Ok(())
}

fn c_remove<C: Component>(o: &mut Object) -> bool {
    !C::REQUIRED && C::clear(o)
}

const fn component<C: Component>() -> ComponentInfo {
    ComponentInfo {
        name: C::NAME,
        type_info: C::type_info,
        required: C::REQUIRED,
        lua: C::LUA,
        get: c_get::<C>,
        get_path: c_get_path::<C>,
        set: c_set::<C>,
        add: c_add::<C>,
        remove: c_remove::<C>,
    }
}

impl Component for Transform {
    const NAME: &'static str = "transform";
    const REQUIRED: bool = true;
    fn load(o: &Object) -> Option<Self> {
        Some(o.transform)
    }
    fn store(self, o: &mut Object) {
        o.transform = self;
    }
    fn clear(_: &mut Object) -> bool {
        false
    }
    fn create() -> Option<Self> {
        Some(Self::default())
    }
}

impl Component for Material {
    const NAME: &'static str = "material";
    const REQUIRED: bool = true;
    fn load(o: &Object) -> Option<Self> {
        Some(o.material)
    }
    fn store(self, o: &mut Object) {
        o.material = self;
    }
    fn clear(_: &mut Object) -> bool {
        false
    }
    fn create() -> Option<Self> {
        Some(Self::default())
    }
}

impl Component for Body {
    const NAME: &'static str = "body";
    const REQUIRED: bool = false;
    fn load(o: &Object) -> Option<Self> {
        o.body.clone()
    }
    fn store(self, o: &mut Object) {
        o.body = Some(self);
    }
    fn clear(o: &mut Object) -> bool {
        o.body.take().is_some()
    }
    fn create() -> Option<Self> {
        Some(Self::default())
    }
}

impl Component for Light {
    const NAME: &'static str = "light";
    const REQUIRED: bool = false;
    const LUA: LuaScope = LuaScope::Handle("light");
    fn load(o: &Object) -> Option<Self> {
        o.light
    }
    fn store(self, o: &mut Object) {
        o.light = Some(self);
    }
    fn clear(o: &mut Object) -> bool {
        o.light.take().is_some()
    }
    fn create() -> Option<Self> {
        Some(Self::default())
    }
}

impl Component for MeshRef {
    const NAME: &'static str = "mesh";
    const REQUIRED: bool = false;
    const WRITABLE: bool = false;
    fn load(o: &Object) -> Option<Self> {
        let mesh = o.mesh?;
        Some(Self { mesh, lod: o.lod.iter().map(|&(distance, mesh)| LodLevel { distance, mesh }).collect() })
    }
    fn store(self, o: &mut Object) {
        // Unreachable while `WRITABLE` is false; kept correct for when meshes get recipes.
        o.mesh = Some(self.mesh);
        o.lod = self.lod.iter().map(|l| (l.distance, l.mesh)).collect();
    }
    fn clear(o: &mut Object) -> bool {
        let had = o.mesh.is_some();
        o.mesh = None;
        o.lod.clear();
        o.local_bounds = Aabb::EMPTY;
        had
    }
    fn create() -> Option<Self> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Destructures `Object` exhaustively: adding a field to `Object` is a compile error here, and then
    /// `OBJECT_FIELDS` must get an entry for it (second half of the test).
    #[test]
    fn every_object_field_has_a_registry_decision() {
        macro_rules! object_fields {
            ($($f:ident),* $(,)?) => {{
                let Object { $($f),* } = Object::empty();
                $( let _ = &$f; )*
                [$(stringify!($f)),*]
            }};
        }
        let mut destructured =
            object_fields!(name, transform, mesh, lod, material, visible, body, light, node_id, parent, children, local_bounds, interp);
        let mut listed: Vec<&str> = OBJECT_FIELDS.iter().map(|(n, _)| *n).collect();
        destructured.sort_unstable();
        listed.sort_unstable();
        assert_eq!(destructured.as_slice(), listed.as_slice(), "OBJECT_FIELDS must list every field of Object, once");
    }
}

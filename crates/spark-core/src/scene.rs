use std::fmt::Write as _;

use glam::{EulerRot, Mat4, Quat, Vec3};
use spark_reflect::{Reflect, ReflectEnum, ReflectError, Value};

use crate::assets::{CubemapId, MeshId};
use crate::camera::Camera;
use crate::color::Color;
use crate::material::Material;
use crate::math::look_rotation;
use crate::mesh::Aabb;
use crate::node_id::NodeId;
use crate::physics::Body;

/// Handle to an object in a [`Scene`]. Stays valid until the object is destroyed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ObjectId {
    index: u32,
    generation: u32,
}

impl ObjectId {
    pub fn index(self) -> u32 {
        self.index
    }

    /// Unique 64-bit value (index + generation), stable while the object lives.
    pub fn to_bits(self) -> u64 {
        ((self.generation as u64) << 32) | self.index as u64
    }

    /// Inverse of [`ObjectId::to_bits`]. The id may refer to a destroyed object.
    pub fn from_bits(bits: u64) -> Self {
        Self { index: bits as u32, generation: (bits >> 32) as u32 }
    }
}

/// Local position, rotation and scale of an [Object] (relative to its parent, or the world).
#[derive(Clone, Copy, Debug, PartialEq, Reflect)]
pub struct Transform {
    /// Position in meters.
    #[reflect(unit = "m", lua = "position")]
    pub position: Vec3,
    /// Rotation, reflected as Euler angles in degrees (pitch, yaw, roll; Y-X-Z order). The scripting API
    /// uses radians for the same angles.
    #[reflect(unit = "deg", lua = "rotation")]
    pub rotation: Quat,
    /// Scale per axis (1 = original size).
    #[reflect(lua = "scale")]
    pub scale: Vec3,
}

impl Transform {
    pub const IDENTITY: Self = Self { position: Vec3::ZERO, rotation: Quat::IDENTITY, scale: Vec3::ONE };
}

impl Default for Transform {
    fn default() -> Self {
        Self::IDENTITY
    }
}

/// The `script` component (ADR 0005, section 4): a Luau script attached to an object, with the values of its
/// `props`. In a document it is the map `{ path = "src/door.luau", props = { open_angle = 90 } }`.
///
/// ```
/// use spark_core::scene::ScriptComponent;
/// use spark_reflect::Value;
///
/// let s = ScriptComponent { path: "src/door.luau".into(), props: vec![("speed".into(), Value::Int(2))] };
/// assert_eq!(ScriptComponent::from_value(&s.to_value()).unwrap(), s);
/// ```
#[derive(Clone, Debug, PartialEq, Default)]
pub struct ScriptComponent {
    /// Game-relative path of the script module (`src/door.luau`).
    pub path: String,
    /// Initial values of the script's props, by name (they override the defaults the script declares).
    pub props: Vec<(String, Value)>,
}

impl ScriptComponent {
    pub fn new(path: &str) -> Self {
        Self { path: path.to_string(), props: Vec::new() }
    }

    /// The document form: a map with `path` and, when there are props, `props`.
    pub fn to_value(&self) -> Value {
        let mut pairs = vec![("path".to_string(), Value::Str(self.path.clone()))];
        if !self.props.is_empty() {
            pairs.push(("props".to_string(), Value::Map(self.props.clone())));
        }
        Value::Map(pairs)
    }

    /// Reads the document form. Unknown keys are errors with did-you-mean suggestions.
    pub fn from_value(v: &Value) -> Result<Self, ReflectError> {
        let Value::Map(pairs) = v else {
            return Err(ReflectError::WrongType { path: String::new(), expected: "map", got: v.type_name() });
        };
        let mut s = Self::default();
        let mut has_path = false;
        for (k, val) in pairs {
            match (k.as_str(), val) {
                ("path", Value::Str(p)) => {
                    s.path = p.clone();
                    has_path = true;
                }
                ("path", other) => {
                    return Err(ReflectError::WrongType { path: "path".into(), expected: "string", got: other.type_name() });
                }
                ("props", Value::Map(p)) => s.props = p.clone(),
                ("props", other) => {
                    return Err(ReflectError::WrongType { path: "props".into(), expected: "map", got: other.type_name() });
                }
                _ => {
                    return Err(ReflectError::UnknownField {
                        ty: "script",
                        path: k.clone(),
                        suggestions: spark_reflect::did_you_mean(k, ["path", "props"]),
                    });
                }
            }
        }
        if !has_path {
            return Err(ReflectError::WrongType { path: "path".into(), expected: "string", got: "none" });
        }
        Ok(s)
    }
}

/// Something in the world: a transform plus an optional mesh.
#[derive(Clone, Debug)]
pub struct Object {
    pub name: String,
    /// Position relative to the parent (or the world if no parent).
    pub transform: Transform,
    pub mesh: Option<MeshId>,
    /// Level of detail: `(distance, mesh)` pairs. From `distance` meters away from the camera on
    /// the renderer draws that mesh instead of `mesh` (the entry with the largest distance that is
    /// reached wins; order does not matter). Picking, physics and bounds use `mesh`.
    pub lod: Vec<(f32, MeshId)>,
    pub material: Material,
    pub visible: bool,
    /// Rigid body simulated by the physics backend (`None` = not physical).
    pub body: Option<Body>,
    /// Light emitted from the object's position (spot lights shine along -Z).
    pub light: Option<Light>,
    /// Script attached to the object (`None` = none). Changed through [`Scene::set_script`] (or set before
    /// [`Scene::add`]) so that the scene can list the scripted objects in spawn order.
    pub(crate) script: Option<ScriptComponent>,
    /// Stable id for commands and scene files, assigned lazily (see [`crate::node_id`]). Cleared by [`Scene::add`].
    pub(crate) node_id: Option<NodeId>,
    pub(crate) parent: Option<ObjectId>,
    /// Direct children (kept in sync by `Scene::set_parent` / `destroy`).
    pub(crate) children: Vec<ObjectId>,
    pub(crate) local_bounds: Aabb,
    /// Fixed-step interpolation state of simulated bodies (see [`Interp`]).
    pub(crate) interp: Option<Interp>,
}

/// Local pose before and after the last fixed (physics) step. The renderer draws the object
/// in between (`Time::alpha`) so motion is smooth on any refresh rate. Teleports (game code
/// changing `position` / `rotation`) are detected and drawn without interpolation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Interp {
    pub prev_position: Vec3,
    pub prev_rotation: Quat,
    pub position: Vec3,
    pub rotation: Quat,
}

impl Object {
    /// Empty object (a group / pivot).
    pub fn empty() -> Self {
        Self {
            name: String::new(),
            transform: Transform::default(),
            mesh: None,
            lod: Vec::new(),
            material: Material::default(),
            visible: true,
            body: None,
            light: None,
            script: None,
            node_id: None,
            parent: None,
            children: Vec::new(),
            local_bounds: Aabb::EMPTY,
            interp: None,
        }
    }

    /// Mesh drawn at `distance` meters from the camera: the [`Object::lod`] entry with the
    /// largest distance not above `distance`, or [`Object::mesh`] when none is reached.
    pub fn mesh_at(&self, distance: f32) -> Option<MeshId> {
        self.lod.iter().filter(|(d, _)| *d <= distance).max_by(|a, b| a.0.total_cmp(&b.0)).map(|(_, m)| *m).or(self.mesh)
    }

    /// Direct children of this object.
    pub fn child_ids(&self) -> &[ObjectId] {
        &self.children
    }

    /// Local matrix drawn this frame: interpolated between the last two fixed steps for
    /// simulated bodies (`alpha` 0..1), the plain local matrix otherwise.
    pub fn render_local_matrix(&self, alpha: f32) -> Mat4 {
        if let Some(i) = &self.interp {
            if i.position == self.transform.position && i.rotation == self.transform.rotation {
                let p = i.prev_position.lerp(i.position, alpha);
                let r = i.prev_rotation.slerp(i.rotation, alpha);
                return Mat4::from_scale_rotation_translation(self.transform.scale, r, p);
            }
        }
        self.local_matrix()
    }

    pub fn parent(&self) -> Option<ObjectId> {
        self.parent
    }

    /// The script attached to this object, if any.
    pub fn script(&self) -> Option<&ScriptComponent> {
        self.script.as_ref()
    }

    /// Stable node id, if one was assigned (see [`crate::World::node_id`]).
    pub fn node_id(&self) -> Option<&NodeId> {
        self.node_id.as_ref()
    }

    /// Mesh bounds in the object's own space (no transform applied).
    pub fn local_bounds(&self) -> Aabb {
        self.local_bounds
    }

    pub fn local_matrix(&self) -> Mat4 {
        Mat4::from_scale_rotation_translation(self.transform.scale, self.transform.rotation, self.transform.position)
    }

    /// Rotates around the object's own X axis.
    pub fn rotate_x(&mut self, angle: f32) {
        self.transform.rotation = (self.transform.rotation * Quat::from_rotation_x(angle)).normalize();
    }

    /// Rotates around the object's own Y axis.
    pub fn rotate_y(&mut self, angle: f32) {
        self.transform.rotation = (self.transform.rotation * Quat::from_rotation_y(angle)).normalize();
    }

    /// Rotates around the object's own Z axis.
    pub fn rotate_z(&mut self, angle: f32) {
        self.transform.rotation = (self.transform.rotation * Quat::from_rotation_z(angle)).normalize();
    }

    pub fn translate(&mut self, delta: Vec3) {
        self.transform.position += delta;
    }

    /// Euler angles in radians (x = pitch, y = yaw, z = roll), applied in Y-X-Z order.
    pub fn euler(&self) -> Vec3 {
        let (y, x, z) = self.transform.rotation.to_euler(EulerRot::YXZ);
        Vec3::new(x, y, z)
    }

    pub fn set_euler(&mut self, angles: Vec3) {
        self.transform.rotation = Quat::from_euler(EulerRot::YXZ, angles.y, angles.x, angles.z);
    }

    /// Turns the object so its -Z axis points at `target` (in parent space).
    pub fn look_at(&mut self, target: Vec3) {
        if target != self.transform.position {
            self.transform.rotation = look_rotation(target - self.transform.position);
        }
    }

    /// Local -Z axis.
    pub fn forward(&self) -> Vec3 {
        self.transform.rotation * Vec3::NEG_Z
    }
}

/// Kind of a [`Light`].
#[derive(Clone, Copy, Debug, PartialEq, ReflectEnum)]
pub enum LightKind {
    /// Shines in all directions.
    #[reflect(name = "point")]
    Point,
    /// Cone along the object's -Z.
    #[reflect(name = "spot")]
    Spot {
        /// Full cone angle in radians (the scripting API uses degrees, 1 to 179).
        #[reflect(
            range = 1.0f32.to_radians()..=179.0f32.to_radians(),
            clamp,
            unit = "rad",
            lua = "angle",
            default = 45f32.to_radians()
        )]
        angle: f32,
        /// 0..1 = blurry edge.
        #[reflect(range = 0.0..=1.0, clamp, step = 0.01, lua = "softness", default = 0.3)]
        softness: f32,
    },
}

/// Point or spot light attached to an object. Reaches exactly `range` meters (smooth falloff).
///
/// The `lua` names of its fields are members of the Luau `obj.light` handle (`obj.light.intensity`), not of the
/// object itself (`obj.color` is the material color).
#[derive(Clone, Copy, Debug, PartialEq, Reflect)]
pub struct Light {
    /// Point or spot (Luau: `light.type`, `"point"` / `"spot"`).
    #[reflect(lua = "type")]
    pub kind: LightKind,
    /// Light color.
    #[reflect(lua = "color")]
    pub color: Color,
    /// Brightness multiplier (0 = off). Negative values are clamped to 0.
    #[reflect(range = 0.0..=f32::MAX, clamp, step = 0.05, lua = "intensity")]
    pub intensity: f32,
    /// Distance in meters where the light has faded out completely. Values below 0.01 are clamped.
    #[reflect(range = 0.01..=f32::MAX, clamp, unit = "m", lua = "range")]
    pub range: f32,
    /// Casts shadows (a cube shadow map, see `RenderSettings::light_shadows`).
    #[reflect(lua = "shadows")]
    pub shadows: bool,
}

impl Default for Light {
    /// What `light(true)` / `add_light` give in scripts: a white point light, intensity 1, range 10 m.
    fn default() -> Self {
        Self::point(Color::WHITE, 1.0, 10.0)
    }
}

impl Light {
    pub const MAX_VISIBLE: usize = 64;
    /// Upper limit of `RenderSettings::light_shadows`.
    pub const MAX_SHADOWED: usize = 8;

    pub fn point(color: Color, intensity: f32, range: f32) -> Self {
        Self { kind: LightKind::Point, color, intensity, range, shadows: false }
    }

    pub fn spot(color: Color, intensity: f32, range: f32, angle: f32, softness: f32) -> Self {
        Self { kind: LightKind::Spot { angle, softness }, color, intensity, range, shadows: false }
    }
}

/// Directional light (the sun).
#[derive(Clone, Copy, Debug, PartialEq, Reflect)]
pub struct Sun {
    /// Direction the light travels (points away from the sun).
    pub direction: Vec3,
    /// Light color.
    pub color: Color,
    /// Brightness multiplier.
    pub intensity: f32,
}

impl Default for Sun {
    /// The sun of a new [`Scene`].
    fn default() -> Self {
        Self { direction: Vec3::new(-0.4, -1.0, -0.3), color: Color::WHITE, intensity: 1.0 }
    }
}

/// Linear distance fog.
#[derive(Clone, Copy, Debug, PartialEq, Reflect)]
pub struct Fog {
    /// Fog color (distant objects fade into it).
    pub color: Color,
    /// Distance in meters where the fog starts.
    #[reflect(unit = "m")]
    pub near: f32,
    /// Distance in meters where objects are fully fogged.
    #[reflect(unit = "m")]
    pub far: f32,
}

impl Default for Fog {
    /// Gray fog from 10 m to 60 m (the scripting API's `fog(color)` distances).
    fn default() -> Self {
        Self { color: Color::GRAY, near: 10.0, far: 60.0 }
    }
}

/// Sky drawn behind every object (instead of the flat `background` color). It can also light
/// the scene (image-based lighting): see [`Sky::lighting`].
#[derive(Clone, Copy, Debug, PartialEq, Reflect)]
pub struct Sky {
    /// Color straight up; the sky fades to `bottom` straight down (the horizon is halfway).
    pub top: Color,
    /// Color straight down.
    pub bottom: Color,
    /// Optional cubemap (photo or painted sky), drawn over the gradient (a runtime handle).
    #[reflect(readonly, transient)]
    pub cubemap: Option<CubemapId>,
    /// How much of the cubemap covers the gradient, 0..1.
    #[reflect(range = 0.0..=1.0, clamp, step = 0.01)]
    pub blend: f32,
    /// Image-based lighting strength. 0 = off (objects use the flat `ambient` color);
    /// above 0 the sky replaces the ambient light (blue from above, ground color from below)
    /// and PBR surfaces reflect it.
    #[reflect(range = 0.0..=f32::MAX, clamp, step = 0.05)]
    pub lighting: f32,
}

impl Sky {
    /// Two-color gradient sky.
    pub fn gradient(top: Color, bottom: Color) -> Self {
        Self { top, bottom, cubemap: None, blend: 1.0, lighting: 0.0 }
    }

    /// Cubemap sky (see [`crate::Assets::load_cubemap`]).
    pub fn cubemap(cubemap: CubemapId) -> Self {
        Self { cubemap: Some(cubemap), ..Self::default() }
    }

    /// Sets the image-based lighting strength (0 = off).
    pub fn with_lighting(mut self, lighting: f32) -> Self {
        self.lighting = lighting.max(0.0);
        self
    }
}

impl Default for Sky {
    fn default() -> Self {
        Self::gradient(Color::rgb(0.38, 0.62, 0.92), Color::rgb(0.22, 0.2, 0.18))
    }
}

#[derive(Clone, Debug)]
struct Slot {
    generation: u32,
    object: Option<Object>,
}

/// All objects plus camera and lighting.
#[derive(Clone, Debug)]
pub struct Scene {
    slots: Vec<Slot>,
    free: Vec<u32>,
    count: usize,
    pub camera: Camera,
    pub sun: Sun,
    pub ambient: Color,
    pub background: Color,
    pub fog: Option<Fog>,
    /// Sky behind everything; `None` = clear to `background`.
    pub sky: Option<Sky>,
    /// Objects that have a script, in the order the scripts were attached (a monotonic sequence number, not the slot
    /// index, so the order does not depend on slot reuse). Sorted by sequence number.
    scripted: Vec<(u64, ObjectId)>,
    script_seq: u64,
    /// Bumped whenever `scripted` changes (the script host compares it to skip work).
    script_epoch: u64,
}

impl Default for Scene {
    fn default() -> Self {
        Self::new()
    }
}

impl Scene {
    pub fn new() -> Self {
        Self {
            slots: Vec::new(),
            free: Vec::new(),
            count: 0,
            camera: Camera::default(),
            sun: Sun::default(),
            ambient: Color::rgb(0.35, 0.38, 0.45),
            background: Color::rgb(0.08, 0.09, 0.12),
            fog: None,
            sky: None,
            scripted: Vec::new(),
            script_seq: 0,
            script_epoch: 0,
        }
    }

    /// Objects with a script as `(sequence number, object)`, ordered by when the script was attached. A script that is
    /// replaced or re-attached gets a new, larger number.
    pub fn scripted(&self) -> &[(u64, ObjectId)] {
        &self.scripted
    }

    /// Changes whenever [`Scene::scripted`] does.
    pub fn script_epoch(&self) -> u64 {
        self.script_epoch
    }

    fn register_script(&mut self, id: ObjectId) {
        self.script_seq += 1;
        self.scripted.push((self.script_seq, id));
        self.script_epoch += 1;
    }

    fn unregister_script(&mut self, id: ObjectId) {
        self.scripted.retain(|(_, o)| *o != id);
        self.script_epoch += 1;
    }

    /// Attaches, replaces (`Some`) or removes (`None`) the script of an object. Returns false if the object does not
    /// exist. Attaching again counts as a new script: the script host restarts it.
    pub fn set_script(&mut self, id: ObjectId, script: Option<ScriptComponent>) -> bool {
        let Some(o) = self.get_mut(id) else { return false };
        let had = o.script.is_some();
        let has = script.is_some();
        o.script = script;
        if had {
            self.unregister_script(id);
        }
        if has {
            self.register_script(id);
        }
        true
    }

    /// Adds an object. Prefer `World::spawn`, which also sets mesh bounds.
    pub fn add(&mut self, mut object: Object) -> ObjectId {
        self.count += 1;
        object.children.clear();
        object.node_id = None; // ids are bound by `World` (a cloned object must not share one)
        let parent = object.parent.filter(|p| self.contains(*p));
        object.parent = parent;
        let id = if let Some(index) = self.free.pop() {
            let slot = &mut self.slots[index as usize];
            slot.object = Some(object);
            ObjectId { index, generation: slot.generation }
        } else {
            self.slots.push(Slot { generation: 0, object: Some(object) });
            ObjectId { index: self.slots.len() as u32 - 1, generation: 0 }
        };
        if let Some(p) = parent {
            self[p].children.push(id);
        }
        if self[id].script.is_some() {
            self.register_script(id);
        }
        id
    }

    /// Removes an object and all its children. Returns false if it did not exist.
    pub fn destroy(&mut self, id: ObjectId) -> bool {
        let Some(parent) = self.get(id).map(|o| o.parent) else { return false };
        if let Some(p) = parent.and_then(|p| self.get_mut(p)) {
            p.children.retain(|c| *c != id);
        }
        // Iterative: deep hierarchies cannot overflow the stack. O(size of the subtree).
        let mut stack = vec![id];
        while let Some(cur) = stack.pop() {
            let slot = &mut self.slots[cur.index as usize];
            if slot.generation != cur.generation {
                continue;
            }
            let Some(o) = slot.object.take() else { continue };
            let scripted = o.script.is_some();
            stack.extend(o.children);
            slot.generation = slot.generation.wrapping_add(1);
            self.free.push(cur.index);
            self.count -= 1;
            if scripted {
                self.unregister_script(cur);
            }
        }
        true
    }

    /// Removes all objects (camera and lighting stay). Every old [`ObjectId`] becomes invalid.
    pub fn clear_objects(&mut self) {
        self.free.clear();
        for (i, slot) in self.slots.iter_mut().enumerate() {
            slot.object = None;
            slot.generation = slot.generation.wrapping_add(1);
            self.free.push(i as u32);
        }
        self.free.reverse();
        self.count = 0;
        if !self.scripted.is_empty() {
            self.scripted.clear();
            self.script_epoch += 1;
        }
    }

    pub fn contains(&self, id: ObjectId) -> bool {
        self.get(id).is_some()
    }

    /// The live object in slot index (what ObjectId::index / the scene dump report), if any.
    pub fn id_of_index(&self, index: u32) -> Option<ObjectId> {
        let s = self.slots.get(index as usize)?;
        s.object.as_ref().map(|_| ObjectId { index, generation: s.generation })
    }

    pub fn get(&self, id: ObjectId) -> Option<&Object> {
        self.slots.get(id.index as usize).filter(|s| s.generation == id.generation).and_then(|s| s.object.as_ref())
    }

    pub fn get_mut(&mut self, id: ObjectId) -> Option<&mut Object> {
        self.slots.get_mut(id.index as usize).filter(|s| s.generation == id.generation).and_then(|s| s.object.as_mut())
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    pub fn iter(&self) -> impl Iterator<Item = (ObjectId, &Object)> {
        self.slots
            .iter()
            .enumerate()
            .filter_map(|(i, s)| s.object.as_ref().map(|o| (ObjectId { index: i as u32, generation: s.generation }, o)))
    }

    /// First object with this name.
    pub fn find(&self, name: &str) -> Option<ObjectId> {
        self.iter().find(|(_, o)| o.name == name).map(|(id, _)| id)
    }

    pub fn children(&self, id: ObjectId) -> Vec<ObjectId> {
        self.get(id).map(|o| o.children.clone()).unwrap_or_default()
    }

    /// Called before every fixed step: remembers the pose of every moving body.
    pub fn begin_interp_step(&mut self) {
        for slot in &mut self.slots {
            let Some(o) = slot.object.as_mut() else { continue };
            let moving = o.body.as_ref().is_some_and(|b| b.kind != crate::physics::BodyKind::Static);
            o.interp = moving.then_some(Interp {
                prev_position: o.transform.position,
                prev_rotation: o.transform.rotation,
                position: o.transform.position,
                rotation: o.transform.rotation,
            });
        }
    }

    /// Called after every fixed step (physics included): stores the new pose of moving bodies.
    pub fn end_interp_step(&mut self) {
        for slot in &mut self.slots {
            let Some(o) = slot.object.as_mut() else { continue };
            if let Some(i) = &mut o.interp {
                i.position = o.transform.position;
                i.rotation = o.transform.rotation;
            }
        }
    }

    /// Number of slots (live + free). Object indices are `0..capacity()`.
    pub fn capacity(&self) -> usize {
        self.slots.len()
    }

    /// World matrix and effective visibility of every object, indexed by `ObjectId::index`, in
    /// one O(n) pass (parents first). Simulated bodies are interpolated by `alpha` (see
    /// [`Object::render_local_matrix`]). The renderer calls this once per frame.
    pub fn compute_world_transforms(&self, alpha: f32, matrices: &mut Vec<Mat4>, visible: &mut Vec<bool>) {
        let n = self.slots.len();
        matrices.clear();
        matrices.resize(n, Mat4::IDENTITY);
        visible.clear();
        visible.resize(n, false);
        let mut stack: Vec<(u32, Mat4, bool)> = Vec::with_capacity(64);
        for (i, slot) in self.slots.iter().enumerate() {
            let Some(o) = &slot.object else { continue };
            if o.parent.is_some() {
                continue;
            }
            stack.push((i as u32, Mat4::IDENTITY, true));
            while let Some((idx, parent_m, parent_vis)) = stack.pop() {
                let Some(o) = self.slots[idx as usize].object.as_ref() else { continue };
                let m = parent_m * o.render_local_matrix(alpha);
                let vis = parent_vis && o.visible;
                matrices[idx as usize] = m;
                visible[idx as usize] = vis;
                for c in &o.children {
                    stack.push((c.index, m, vis));
                }
            }
        }
    }

    /// Attaches `child` to `parent` (keeps the child's local transform), or detaches with `None`.
    pub fn set_parent(&mut self, child: ObjectId, parent: Option<ObjectId>) -> Result<(), String> {
        if !self.contains(child) {
            return Err("set_parent: child object does not exist".into());
        }
        if let Some(p) = parent {
            if !self.contains(p) {
                return Err("set_parent: parent object does not exist".into());
            }
            let mut cur = Some(p);
            while let Some(c) = cur {
                if c == child {
                    return Err("set_parent: would create a cycle (parent is a child of this object)".into());
                }
                cur = self.get(c).and_then(|o| o.parent);
            }
        }
        let old = self[child].parent;
        if old == parent {
            return Ok(());
        }
        if let Some(o) = old.and_then(|o| self.get_mut(o)) {
            o.children.retain(|c| *c != child);
        }
        if let Some(p) = parent {
            self[p].children.push(child);
        }
        self[child].parent = parent;
        Ok(())
    }

    /// Object-to-world matrix including all parents.
    pub fn world_matrix(&self, id: ObjectId) -> Mat4 {
        let mut m = Mat4::IDENTITY;
        let mut cur = Some(id);
        let mut depth = 0;
        while let Some(c) = cur {
            let Some(o) = self.get(c) else { break };
            m = o.local_matrix() * m;
            cur = o.parent;
            depth += 1;
            if depth > self.slots.len() {
                break; // cycle guard (set_parent prevents cycles)
            }
        }
        m
    }

    pub fn world_position(&self, id: ObjectId) -> Vec3 {
        self.world_matrix(id).transform_point3(Vec3::ZERO)
    }

    /// World-space bounds of the object's own mesh (children not included).
    pub fn world_bounds(&self, id: ObjectId) -> Aabb {
        match self.get(id) {
            Some(o) => o.local_bounds.transform(&self.world_matrix(id)),
            None => Aabb::EMPTY,
        }
    }

    /// Visible itself and all parents visible.
    pub fn is_visible(&self, id: ObjectId) -> bool {
        let mut cur = Some(id);
        while let Some(c) = cur {
            match self.get(c) {
                Some(o) if o.visible => cur = o.parent,
                _ => return false,
            }
        }
        true
    }

    /// Moves the object by a world-space offset (works with parents).
    pub fn translate_world(&mut self, id: ObjectId, delta: Vec3) {
        let local = match self.get(id).and_then(|o| o.parent) {
            Some(p) => self.world_matrix(p).inverse().transform_vector3(delta),
            None => delta,
        };
        if let Some(o) = self.get_mut(id) {
            o.transform.position += local;
        }
    }

    /// Moves the object up/down so its bottom rests on top of `target`. X/Z stay the same.
    pub fn place_on(&mut self, id: ObjectId, target: ObjectId) -> Result<(), String> {
        let top = self.world_bounds(target);
        if top.is_empty() {
            return Err("place_on: target has no mesh (no bounds)".into());
        }
        if !self.contains(id) {
            return Err("place_on: object does not exist".into());
        }
        let own = self.world_bounds(id);
        let bottom = if own.is_empty() { self.world_position(id).y } else { own.min.y };
        self.translate_world(id, Vec3::new(0.0, top.max.y - bottom, 0.0));
        Ok(())
    }

    /// Machine readable dump (JSON) of the camera and every object: for tests, CI and AI tools.
    pub fn dump_json(&self) -> String {
        fn esc(s: &str) -> String {
            let mut o = String::with_capacity(s.len() + 2);
            for ch in s.chars() {
                match ch {
                    '"' => o.push_str("\\\""),
                    '\\' => o.push_str("\\\\"),
                    c if (c as u32) < 0x20 => o.push_str(&format!("\\u{:04x}", c as u32)),
                    c => o.push(c),
                }
            }
            o
        }
        fn v(x: Vec3) -> String {
            let f = |n: f32| if n.is_finite() { format!("{n:.4}") } else { "null".into() };
            format!("[{},{},{}]", f(x.x), f(x.y), f(x.z))
        }
        let c = &self.camera;
        let mut out = format!(
            "{{\"camera\":{{\"position\":{},\"forward\":{},\"fov\":{:.2},\"ortho\":{}}},\"objects\":[",
            v(c.position),
            v(c.forward()),
            c.fov,
            c.ortho.map_or("null".into(), |o| format!("{o:.3}"))
        );
        let (mut mats, mut vis) = (Vec::new(), Vec::new());
        self.compute_world_transforms(1.0, &mut mats, &mut vis);
        for (n, (id, o)) in self.iter().enumerate() {
            let i = id.index() as usize;
            let b = o.local_bounds.transform(&mats[i]);
            let e = o.euler();
            let _ = write!(
                out,
                "{}{{\"id\":{},\"node_id\":{},\"name\":\"{}\",\"parent\":{},\"mesh\":{},\"position\":{},\"world_position\":{},\"rotation_deg\":{},\"scale\":{},\"visible\":{},\"bounds\":{},\"body\":{},\"light\":{}}}",
                if n > 0 { "," } else { "" },
                id.index(),
                o.node_id.as_ref().map_or("null".into(), |n| format!("\"{n}\"")),
                esc(&o.name),
                o.parent.map_or("null".into(), |p| p.index().to_string()),
                o.mesh.map_or("null".into(), |m| m.0.to_string()),
                v(o.transform.position),
                v(mats[i].w_axis.truncate()),
                v(Vec3::new(e.x.to_degrees(), e.y.to_degrees(), e.z.to_degrees())),
                v(o.transform.scale),
                vis[i],
                if b.is_empty() { "null".into() } else { format!("[{},{}]", v(b.min), v(b.max)) },
                o.body.as_ref().map_or("null".into(), |b| format!("{{\"kind\":\"{}\",\"velocity\":{}}}", b.kind.name(), v(b.velocity))),
                o.light.is_some()
            );
        }
        out.push_str("]}");
        out
    }

    /// Human/AI readable dump of the whole scene.
    pub fn dump(&self) -> String {
        let mut out = String::new();
        let c = &self.camera;
        let _ = writeln!(
            out,
            "Scene: {} objects | camera pos {} forward {} fov {:.0}",
            self.count,
            fmt_v(c.position),
            fmt_v(c.forward()),
            c.fov
        );
        for (id, o) in self.iter().filter(|(_, o)| o.parent.is_none()) {
            self.dump_object(&mut out, id, o, 0);
        }
        out
    }

    fn dump_object(&self, out: &mut String, id: ObjectId, o: &Object, depth: usize) {
        let name = if o.name.is_empty() { format!("#{}", id.index) } else { format!("\"{}\" #{}", o.name, id.index) };
        let e = o.euler();
        let b = self.world_bounds(id);
        let bounds = if b.is_empty() { "none".to_string() } else { format!("{}..{}", fmt_v(b.min), fmt_v(b.max)) };
        let _ = writeln!(
            out,
            "{}- {} mesh={} pos {} rot(deg) {} scale {} world-bounds {}{}",
            "  ".repeat(depth),
            name,
            o.mesh.map(|m| m.0.to_string()).unwrap_or_else(|| "-".into()),
            fmt_v(o.transform.position),
            fmt_v(Vec3::new(e.x.to_degrees(), e.y.to_degrees(), e.z.to_degrees())),
            fmt_v(o.transform.scale),
            bounds,
            if o.visible { "" } else { " HIDDEN" }
        );
        if let Some(l) = &o.light {
            let kind = match l.kind {
                LightKind::Point => "point".to_string(),
                LightKind::Spot { angle, .. } => format!("spot {:.0}deg", angle.to_degrees()),
            };
            let _ = writeln!(out, "{}  light: {kind} range {:.1} intensity {:.2}", "  ".repeat(depth), l.range, l.intensity);
        }
        for child in self.children(id) {
            if let Some(co) = self.get(child) {
                self.dump_object(out, child, co, depth + 1);
            }
        }
    }
}

fn fmt_v(v: Vec3) -> String {
    format!("({:.2}, {:.2}, {:.2})", v.x, v.y, v.z)
}

impl std::ops::Index<ObjectId> for Scene {
    type Output = Object;
    fn index(&self, id: ObjectId) -> &Object {
        self.get(id).unwrap_or_else(|| panic!("object #{} does not exist (destroyed?)", id.index))
    }
}

impl std::ops::IndexMut<ObjectId> for Scene {
    fn index_mut(&mut self, id: ObjectId) -> &mut Object {
        self.get_mut(id).unwrap_or_else(|| panic!("object #{} does not exist (destroyed?)", id.index))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lod_picks_the_farthest_reached_level() {
        let mut o = Object::empty();
        o.mesh = Some(MeshId(0));
        o.lod = vec![(40.0, MeshId(2)), (10.0, MeshId(1))]; // any order
        assert_eq!(o.mesh_at(0.0), Some(MeshId(0)));
        assert_eq!(o.mesh_at(9.9), Some(MeshId(0)));
        assert_eq!(o.mesh_at(10.0), Some(MeshId(1)));
        assert_eq!(o.mesh_at(39.0), Some(MeshId(1)));
        assert_eq!(o.mesh_at(1000.0), Some(MeshId(2)));
        assert_eq!(Object::empty().mesh_at(5.0), None);
    }
    use crate::physics::{Body, BodyKind};

    #[test]
    fn clear_objects_invalidates_ids() {
        let mut s = Scene::new();
        let a = s.add(Object::empty());
        s.clear_objects();
        assert!(!s.contains(a));
        let b = s.add(Object::empty());
        assert_eq!(b.index(), a.index());
        assert!(!s.contains(a) && s.contains(b));
    }

    #[test]
    fn hierarchy_index_and_destroy() {
        let mut s = Scene::new();
        let root = s.add(Object::empty());
        let mut last = root;
        for _ in 0..10_000 {
            let mut o = Object::empty();
            o.transform.position = Vec3::X;
            o.parent = Some(last);
            last = s.add(o);
        }
        assert_eq!(s.children(root).len(), 1);
        let (mut m, mut v) = (Vec::new(), Vec::new());
        s.compute_world_transforms(1.0, &mut m, &mut v);
        assert!((m[last.index() as usize].w_axis.x - 10_000.0).abs() < 1e-2);
        assert!((s.world_position(last).x - m[last.index() as usize].w_axis.x).abs() < 1e-2);
        let other = s.add(Object::empty());
        s.set_parent(last, Some(other)).unwrap();
        assert_eq!(s.children(other), vec![last]);
        assert!(s.destroy(root)); // deep chain: no stack overflow
        assert_eq!(s.len(), 2);
        let json = s.dump_json();
        assert!(json.starts_with("{\"camera\"") && json.ends_with("]}"));
        assert!(s.destroy(other));
        assert!(s.is_empty());
    }

    #[test]
    fn interpolation_and_teleport() {
        let mut s = Scene::new();
        let mut o = Object::empty();
        o.body = Some(Body::dynamic());
        let id = s.add(o);
        s.begin_interp_step();
        s[id].transform.position = Vec3::new(1.0, 0.0, 0.0); // what physics would do
        s.end_interp_step();
        let half = s[id].render_local_matrix(0.5).w_axis.x;
        assert!((half - 0.5).abs() < 1e-5);
        s[id].transform.position = Vec3::new(10.0, 0.0, 0.0); // teleport by game code: no smear
        assert_eq!(s[id].render_local_matrix(0.5).w_axis.x, 10.0);
    }

    #[test]
    fn destroyed_object_returns_none() {
        let mut s = Scene::new();
        let a = s.add(Object::empty());
        assert!(s.get(a).is_some());
        assert!(s.destroy(a));
        assert!(s.get(a).is_none());
        assert!(s.get_mut(a).is_none());
    }

    #[test]
    fn set_parent_cycle_detected() {
        let mut s = Scene::new();
        let a = s.add(Object::empty());
        let b = s.add(Object::empty());
        s.set_parent(b, Some(a)).unwrap();
        // a is parent of b, so making a a child of b would create a cycle
        let err = s.set_parent(a, Some(b));
        assert!(err.is_err());
        assert!(err.unwrap_err().contains("cycle"));
    }

    #[test]
    fn interpolation_teleport_detection() {
        let mut s = Scene::new();
        let mut obj = Object::empty();
        obj.body = Some(Body { kind: BodyKind::Dynamic, ..Default::default() });
        let id = s.add(obj);
        s.begin_interp_step();
        s.end_interp_step();
        // Teleport: change position after end_interp_step
        s[id].transform.position = Vec3::new(100.0, 0.0, 0.0);
        // render_local_matrix should NOT interpolate (teleport detected)
        let m = s[id].render_local_matrix(0.5);
        let p = m.w_axis.truncate();
        assert_eq!(p, Vec3::new(100.0, 0.0, 0.0));
    }

    #[test]
    fn interpolation_normal_motion() {
        let mut s = Scene::new();
        let mut obj = Object::empty();
        obj.body = Some(Body { kind: BodyKind::Dynamic, ..Default::default() });
        obj.transform.position = Vec3::new(0.0, 0.0, 0.0);
        let id = s.add(obj);
        s.begin_interp_step();
        // Simulate physics moving the object
        s[id].transform.position = Vec3::new(10.0, 0.0, 0.0);
        s.end_interp_step();
        // At alpha=0.5, should be at (5, 0, 0)
        let m = s.get(id).unwrap().render_local_matrix(0.5);
        let p = m.w_axis.truncate();
        assert!((p.x - 5.0).abs() < 1e-3, "expected 5.0, got {}", p.x);
    }
}

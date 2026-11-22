# ADR 0005: `Object` becomes node + typed components; every change is a `Command`; scripts attach to objects

Status: proposed (Stage C). Depends on ADR 0003 (files) and ADR 0004 (registry). Roadmap: Stage C; design: `analysis/10-target-architecture.md` 10.2, 10.3, 10.5; `analysis/06-editor-and-human-workflow.md` 6.4.

## Context

`Object` (`spark-core/src/scene.rs`) is one flat struct: `name, position, rotation, scale, mesh, lod, material, visible, body, light` plus private `parent, children, local_bounds, interp`. `Scene` is a generational slot arena (`ObjectId {index, generation}`); camera, sun, ambient, fog, sky are fields of `Scene`, not objects. The Luau `Obj(ObjectId)` userdata (`spark-script/src/types.rs`) reads and writes through `World` (`Rc<RefCell<World>>` swapped around every call) with one hand-written closure per field and flattens sub-structs into one namespace (`obj.color` is `material.color`, `obj.velocity` is `body.velocity`). Mutations come from three unrelated paths with three validation styles: Luau (`types.rs`, `physics.rs`, `light.rs`), devtools (`spark/src/devtools.rs::apply_set_object`, 5 fields), and nothing else (the editor inspector is read-only). There is no undo, no addressable stable id, and a script error stops the whole script host.

Non-negotiable: the current Luau API (`obj.position`, `obj.color`, `obj:on`, `spawn{...}`, `physics`, all examples and the script contract tests under `crates/spark-script/tests/`) keeps working unchanged. Not a goal: an ECS. The engine keeps "object = node + a small fixed set of components" (Godot-like, analysis 10.2); hot loops (renderer, physics sync) keep direct typed access.

## Decision

### 1. Component model: typed fields, closed set, Luau API untouched

An object is a **node** (name, visibility, hierarchy, stable id) plus **components** stored as typed fields, not a dynamic map. Each component is a struct deriving `Reflect` (ADR 0004) and is listed in the registry. Mapping from today's `Object`:

| Component (registry name) | Presence | Today's `Object` fields / Luau names that map to it |
|---|---|---|
| node (not a component) | always | `name`, `visible`, `parent`/`children` (private, via `set_parent`), new `node_id` (stable string id, assigned lazily) |
| `transform` | always | `position`, `rotation`, `scale`; Luau `position`, `x`, `y`, `z`, `rotation` (radians, euler YXZ), `scale`; methods `rotate_*`, `translate`, `look_at`, `forward`... stay methods on `Object` |
| `mesh` | optional | `mesh: Option<MeshId>`, `lod`; Luau `mesh`, `lod`. New: a serializable recipe (`MeshSource`: primitive + params, asset path + node, or runtime-only) recorded by `Assets` when a mesh is created; `local_bounds` stays a derived cache, not a component |
| `material` | always (meaningful with a mesh) | the `Material` struct: `color, texture, tiling, unlit, shader, data, blend, double_sided, cast_shadow, pbr, metallic, roughness, emissive, normal_map, orm_map`; Luau flat `color, material, blend, double_sided, cast_shadow, pbr, metallic, roughness, emissive, normal_map, orm_map, shader, data`, `obj:paint()` |
| `body` | optional | `body: Option<Body>`; Luau `body`, `velocity, angular_velocity, mass, gravity_scale, sensor`, `add_body/remove_body/impulse/force/torque` |
| `light` | optional | `light: Option<Light>`; Luau `light` (a `LightRef` handle) |
| `camera` | optional (new) | today a singleton `Scene.camera`; see section 5 |
| `script` | optional (new) | none today; section 4 |
| `tags` | optional (new) | none today; additive Luau API `obj.tags`, `obj:has_tag(t)`, `find_all_tagged(t)` |
| runtime state, not components | | `interp` (fixed-step interpolation), animation state (`World.animator`), listeners/timers owned via `owner` (`Scheduler`/`Events`) |

How the Luau API keeps working: the `lua = "..."` attribute on a reflected field (ADR 0004) is the flat name. The generated binding resolves `obj.color` to (`material`, `color`). Names, types, error messages and the strict "did you mean" behavior (`SPK` codes) do not change; the existing script contract tests are the acceptance test. Methods (`rotate_y`, `on`, `emit`, `play`, ...) are not reflected and stay hand-written.

What changes internally, in order of risk:

1. `Object.position/rotation/scale` move into a `Transform` struct (`object.transform.position`). Mechanical, compiler-guided rename (about 210 mentions in `crates/`, including `Camera`, which keeps its own fields). No logic change. Done once, in its own PR.
2. `Material`, `Body`, `Light`, `Camera` get `#[derive(Reflect)]` as they are (their fields are already public); no layout change.
3. The registry exposes `ComponentInfo { get, set, add, remove }` accessors over `Object`; Luau, files, inspector and commands go through `set(path, Value)`.
4. Luau scalar fields are generated from the registry; hand-written closures are deleted group by group; composite handles (`body`, `light`, `material`, `mesh`, `lod`, `parent`) keep hand-written adapters.
5. `Object` gains `node_id: Option<NodeId>`, `script: Option<ScriptComponent>`, `tags: Vec<Tag>`; `World` gets `HashMap<NodeId, ObjectId>`. Spawning from Luau does not pay for ids: `NodeId` is assigned the first time something needs it (command, dump, save).

`Scene` storage (generational slots, O(n) world-transform pass, `ObjectId`) is **not** changed in Stage C. Physics and render keep reading typed fields. Dirty flags/caching (10.6) are a separate task.

### 2. Commands: the only way a document changes

Live in `spark-core` (module `command`), no GPU, no serde. Serialization to JSON/TOML is done by the crates that own those dependencies via `Value`.

```rust
pub struct NodeId(Arc<str>);                      // stable id from ADR 0003

pub enum Command {
    /// Create a subtree (ids inside are kept, so later commands in the history still resolve).
    Spawn { subtree: SceneDoc, parent: Option<NodeId>, index: Option<usize> },
    /// Delete a node and its children.
    Delete { node: NodeId },
    /// `path` = "transform.position", "body.mass", "material.color", "name", "visible", "script.props.speed".
    SetProperty { node: NodeId, path: String, value: Value },
    Reparent { node: NodeId, parent: Option<NodeId>, index: Option<usize> },
    AddComponent { node: NodeId, component: String, value: Option<Value> }, // None = defaults
    RemoveComponent { node: NodeId, component: String },
    /// All or nothing: on the first error the already applied part is rolled back through its inverses.
    Batch(Vec<Command>),
}

pub struct Applied {
    pub inverse: Command,        // value-based, restores the exact previous state (Delete -> Spawn of the captured subtree)
    pub changed: Vec<NodeId>,    // for the editor / devtools to refresh
}

pub enum CommandError { NoSuchNode(NodeId), Reflect(ReflectError), Cycle, NoSuchComponent { .. }, ... } // -> SparkError codes

impl World {
    pub fn apply(&mut self, cmd: &Command) -> Result<Applied, CommandError>;
}

/// Undo/redo for one document (editor), also usable as the agent's command journal.
pub struct History { /* undo: Vec<Entry>, redo: Vec<Entry>, limit */ }
pub struct Entry { pub label: String, pub forward: Command, pub inverse: Command, pub merge_key: Option<(NodeId, String)> }
impl History {
    pub fn record(&mut self, cmd: Command, applied: Applied);   // consecutive SetProperty with the same merge_key coalesce (gizmo drag = one undo step)
    pub fn undo(&mut self, world: &mut World) -> Result<(), CommandError>;
    pub fn redo(&mut self, world: &mut World) -> Result<(), CommandError>;
}
```

Rules: commands address nodes by `NodeId` (never `ObjectId`); values are validated by the registry (type, range, finite); applying is deterministic (same document + same commands = same document, which is what the test and the agent journal rely on). The live `Scene` inside `World` **is** the document at run time; the file model (`SceneDoc`) is its serializable form (`capture`/`instantiate`, ADR 0003). The editor viewport renders a live `World`, so commands apply to it and `save` captures it. Consumers:

- **Editor**: every inspector/hierarchy/gizmo action is a `Command` through `History`.
- **Devtools** (`spark-devtools`): protocol version 4 adds `EditorCommand::Command { json }` carrying the JSON form of a `Command`; `SetObject` stays for one protocol version as sugar that expands to a `Batch` of `SetProperty`; the game applies via `World::apply`. Runtime changes are not persisted unless the editor saves.
- **MCP** (`spark mcp`): `scene_apply(commands)` takes the same JSON, applies it to a scene file (headless `World`) or to an attached live game, returns the per-command result and the inverse list; `scene_read` returns the `SceneDoc` as TOML or JSON. The agent's session journal is a `History` that can roll back each operation.
- **Luau**: the script API does not go through `Command` (hot path, immediate semantics, not undoable); `scene.load/save` and `spawn(prefab)` use `instantiate`/`capture`.

### 3. Prefabs and the file API in terms of commands

`spawn("prefabs/enemy.prefab", { position = ... })` = `instantiate` + `SetProperty` for the given props on the root (same `SpawnProps` as today, validated through the registry). `scene.load(path)` = `instantiate` into the current world (additive; `scene.load(path, { replace = true })` clears first). `scene.save(path)` = `capture` + ADR 0003 writer. Loading never goes through `History` (not undoable).

### 4. Scripts on objects and per-object isolation

`script` component: `{ path: "src/door.luau", props: Map }` (file: `script = "..."` and `[node.script.props]`, see ADR 0003).

```lua
local Door = {}
Door.props = { open_angle = 90, speed = 2 }     -- defaults; types inferred from the values
function Door:start() self.closed = self.rotation end
function Door:update(dt) ... end
function Door:on_collision(other, info) ... end
return Door
```

- The module is loaded once per path (through the existing `require`/sandbox machinery; `lua.sandbox(true)` and the strict globals of ADR 0002 apply unchanged) and is cached; each object gets an instance table. `self.object` is the `Obj`; reads/writes of `Object` members on `self` forward to the `Obj` (so `self.rotation` works as in the design sketch); any other key is instance state. A prop whose name collides with an `Object` member is a load error.
- `props`: scene values override the module defaults; unknown prop name in a file is an error with did-you-mean; the inspector lists them as `script.props.<name>` (owned `DynField` list, ADR 0004).
- Callbacks: `start(self)`, `update(self, dt)`, `fixed_update(self, dt)`, `late_update(self, dt)`, `on_collision(self, other, info)`, `on_destroy(self)`, later `on_reload(self, old_state)`. Order: the global `update` first, then object scripts in spawn order (monotonic sequence, not slot index, so it is deterministic and independent of slot reuse). `main.luau` with global callbacks stays the simple path and keeps its current "error stops the host" behavior.
- **Isolation**: every object callback is invoked through a guarded call. An error (including the watchdog/interrupt and per-call memory failures) marks only that object `ScriptState::Failed { callback, error }`, its script callbacks are skipped from then on, the error is reported once as a `SparkError` (code, object name and `NodeId` in the location, hint) to the console, `check --json`-style output, and `GameMessage::ScriptError` extended with the node id; the editor shows a marker on the node. Everything else keeps running. Failed state is cleared by hot reload of the module, by a `SetProperty` on `script.path`, or by a restart. Listeners/timers the dead object registered with `owner = self.object` stay owned (and are removed on destroy as today). Limits that are per game (1 GiB memory, 5 s watchdog per callback, ADR 0002) are unchanged.

### 5. Camera and environment

- `environment` (sun, ambient, background, fog, sky) is a scene-level singleton in the file, mapped to `Scene.sun/ambient/background/fog/sky`; it is reflected like a component (`EnvironmentInfo`) and addressable by commands as node-less path `environment.*` (open question: reserved node id `"@environment"`).
- `camera` is a component on an object (`fov, near, far, ortho, active`). At runtime `Scene.camera` stays the render source. If an active camera node exists, the `camera` Luau global proxies to that node's transform and camera component (so `camera.position = ...` moves the node and scripts and the file agree); if none exists, behavior is exactly today's. This is the one place where the Luau API is re-plumbed, so it is the last step of the model work and has its own tests.

## Consequences

- The Luau API, examples and tests do not change; the contract tests prove it step by step. Rust callers change once (the `transform` rename).
- Validation is uniform: Luau, devtools, editor, MCP and files all go through `ReflectError`, so the Stage A `SPK` code table gains codes for command errors instead of new ad-hoc strings.
- Undo/redo is value-based (inverse commands), not snapshot-based: memory cost proportional to the change; `Delete` of a large subtree stores the subtree once.
- A closed component set means a user cannot add a custom component type in Rust or Luau; custom per-object data is `script.props` and instance state. This is intended for Stage C-D. Plugin components would need a new ADR.
- Two representations (live `Scene`, `SceneDoc`) must stay in sync: `capture(instantiate(doc)) == doc` is a test, and so is `instantiate(capture(world))` structural equality.
- Per-object scripts add an `Object` field and a per-frame loop over script nodes; with zero script nodes the cost is one empty-list check.
- Lazy `NodeId`s mean objects created by code have no id in a saved file until the first capture, where ids are generated randomly; saving a code-built scene twice yields different ids unless the ids were kept in the live objects (they are: assigned once, kept for the session).
- `docs/spark.d.luau` `Object` block, `SpawnProps`, `BodySpec`, `LightSpec` become generated (ADR 0004); new API additions (`tags`, `scene.*`, `spawn(prefab)`) are additive.

## Alternatives rejected

- **Real ECS (bevy_ecs/hecs/own).** Gives dynamic components and cache-friendly queries, but changes every access path in render/physics/script, makes the Luau handle story harder (entity ids, archetype moves), and is the "become Bevy" outcome the target architecture rules out. A closed typed component set is enough for the features planned and keeps the hot loops as they are.
- **Dynamic component map on `Object` (`HashMap<TypeId, Box<dyn Component>>`).** Flexible for plugins, but slows hot paths (renderer, physics sync, interpolation), needs downcasting, and is unnecessary while the set is closed.
- **Leave `Object` flat and reflect it as one big type.** Cheapest, but the registry then has no notion of components, so files, inspector sections, `AddComponent/RemoveComponent` and prefab overrides (`component.field`) would all need a second grouping layer.
- **Snapshot-based undo** (clone the scene before each edit). Trivial, but O(scene) per edit, cannot coalesce drags or be shipped over devtools/MCP as a small inverse, and conflicts with the agent journal.
- **Closure-based commands** (`Box<dyn FnMut>` with undo closures). Not serializable, so devtools and MCP would need a second command representation; the enum is the wire format.
- **Making the editor the owner of a separate document** and pushing diffs to the game. Two state copies and a sync protocol; analysis 6.5 says the file is the single source of truth and the viewport renders the same engine, so commands apply to the live `World` and are captured to the file.
- **Script isolation by one Lua state per object.** Strong isolation but expensive (memory per state, no shared modules/globals, cross-object calls need marshalling). A guarded call per callback on the shared sandboxed state gives the needed "one object fails, the game continues" at near zero cost; a malicious mod is covered by ADR 0002 limits, not by this.

## Migration plan

Small independent steps, one PR each; after every step all existing tests and examples pass unchanged (`cargo test --workspace`, `spark check` on examples, goldens). The checklist with files and proving tests is `analysis/stage-c-plan.md` (steps C-1 ... C-18).

| Step | Summary |
|---|---|
| C-1 | `spark-reflect`: `Value`, `FieldInfo`, `TypeInfo`, `Reflect`, errors, did-you-mean. No engine changes |
| C-2 | `spark-reflect-derive`: `#[derive(Reflect)]`, `ReflectEnum`, attributes |
| C-3 | Derive on `Material`, `Body`, `Shape`, `BodyKind`, `Light`, `LightKind`, `Camera`, `Sun`, `Fog`, `Sky` (no behavior change) |
| C-4 | `Object.transform` regroup (mechanical) |
| C-5 | Component registry over `Object`, `Object::get_prop/set_prop`, exhaustiveness test |
| C-6 | Luau: did-you-mean field list from the registry (delete field part of `OBJ_MEMBERS`) |
| C-7 | Luau: generated bindings for transform/name/visible (+ benchmark gate) |
| C-8 | Luau: generated bindings for material |
| C-9 | Luau: generated bindings for body/light scalars; delete the dead closures and `PROPS` |
| C-10 | `spark.d.luau` generated component block + `api.json` `components` |
| C-11 | Mesh recipes in `Assets` |
| C-12 | `NodeId`, `Command`, `World::apply`, `History` |
| C-13 | Devtools protocol 4: `Command` message, `SetObject` as shim |
| C-14 | `SceneDoc`, `capture`/`instantiate`, `spark-scene` crate: `.scene` v1 read/write, migrations |
| C-15 | Prefabs + overrides; Luau `scene.load/save`, `spawn(prefab)` |
| C-16 | `script` component, `props`, per-object isolation |
| C-17 | `tags`; camera/environment mapping |
| C-18 | Hot reload of one module; MCP `scene_read`, `scene_apply`, `attach` (roadmap items; own short ADR if the protocol needs one) |

## C-13 decision: `Restore` and exact inverses stay in the game process

`Command::Restore` (and the `Spawn` that undoes a `Delete`) carries exact typed state: a quaternion bit for bit, texture and mesh handles, runtime-only meshes that have no recipe. The registry `Value` form cannot hold that (rotations go through Euler angles, handles are not values), so encoding it would make undo lossy. Decision: such inverses have **no JSON form**. The game keeps them (the last 256, in `crates/spark/src/devtools.rs`) and `CommandResult.inverse` is `{"op":"restore","token":N}`; sending that command back applies the stored one, also inside a `batch`. Everything else (set, reparent, add/remove component, delete of a spawn, and any command the editor writes itself) is plain JSON and survives a game restart; tokens do not (`unknown_token` error). An editor that needs undo across restarts uses the scene file (C-14), not devtools.

Wire details: `EditorCommand::Command { request, json }` is answered by `GameMessage::CommandResult { request, ok, inverse?, changed[], resolved?, error?{code,message,path?,suggestions[]} }`. Values use a natural JSON form (`spark_devtools::command_json`): arrays of 2 to 4 numbers are vectors/colors, `{"$variant":...}` is an enum, JSON integers are `Int`; because a float field rejects an integer, the game retries a `wrong_type` failure once with whole numbers widened to floats. `SetObject` is kept (protocol 2 and 3 editors) as a batch of `SetProperty` built in the game; the editor still sends it.

# ADR 0004: an own small reflection crate (`spark-reflect`) with a derive, not `bevy_reflect`

Status: proposed (Stage C). Amends ADR 0001 (see "Relation to ADR 0001"). Roadmap: Stage C; design: `analysis/10-target-architecture.md` 10.1.

## Context

Today the same fact about an object field is written by hand in at least six places. For `Object.metallic`: `spark-core/src/material.rs` (field + clamping in `with_metallic`), `spark-script/src/types.rs` (`add_field_method_get/set`, the `PROPS` string, `apply_props`, the `OBJ_MEMBERS` list used for did-you-mean), `docs/spark.d.luau` (`Object` and `SpawnProps`), `docs/SPARK_API.md`, `spark-devtools` (`SetObject` has exactly 5 fields, `ObjectInfo` another list), `Scene::dump_json`, and the editor inspector (read-only). They drift (ADR 0001 only guards two of them with tests).

Consumers that must be driven by one description:

1. Luau bindings: field read/write with type and range checks, and did-you-mean for unknown fields (today `OBJ_MEMBERS`, about 70 names mixing fields and methods);
2. inspector UI: label, widget (drag/slider/checkbox/color/asset picker/enum dropdown), range, step, unit, default (reset button), doc tooltip, read-only;
3. scene (de)serialization (ADR 0003): names, defaults (omit when default), enums, asset fields;
4. `docs/spark.d.luau` / `api.json` / `llms.txt` generation for the component part of the API;
5. devtools `SetObject` / the command model (ADR 0005): `SetProperty { node, path, value }` with validation.

Constraints: `spark-core` is GPU-free, `#![forbid(unsafe_code)]`, MSRV 1.85, and has no serde dependency; build time matters (dev profile builds dependencies at opt-level 3); the runtime does not need type erasure over arbitrary user types (components are a closed set, user data lives in script `props`). `syn`, `quote`, `proc-macro2` and `serde_derive` are already in `Cargo.lock`; `bevy_reflect` and `ron` are not.

## Decision

Write `spark-reflect` (runtime library, no engine knowledge, no GPU, no serde) and `spark-reflect-derive` (proc-macro). Closed set of component types registered explicitly in `spark-core`, no global constructors / `inventory`.

### Minimal shapes

```rust
// spark-reflect/src/lib.rs
/// Dynamic value: what the inspector, files, devtools and Luau exchange.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    Vec2([f32; 2]),
    Vec3([f32; 3]),
    Vec4([f32; 4]),
    Color([f32; 4]),                       // linear RGBA
    Enum { variant: String, fields: Vec<(String, Value)> }, // unit variants: fields empty
    List(Vec<Value>),
    Map(Vec<(String, Value)>),             // ordered; used by `script.props`
    NodeRef(String),                       // stable node id
    Asset(String),                         // project-relative path, `#fragment` allowed
    None,                                  // Option::None
}

pub enum FieldType {
    Bool, Int, Float, Str, Vec2, Vec3, Vec4, Color,
    Enum(&'static EnumInfo),
    Struct(&'static TypeInfo),
    Option(&'static FieldType),
    List(&'static FieldType),
    NodeRef,
    Asset(AssetKind),                      // Mesh, Texture, Sound, Script, Prefab, Model
}

pub struct FieldInfo {
    pub name: &'static str,                // file / inspector / path name
    pub ty: FieldType,
    pub doc: &'static str,                 // from the /// comment
    pub default: fn() -> Value,            // from Default or #[reflect(default = ...)]
    pub range: Option<(f64, f64)>,         // inclusive; None = unbounded; floats must be finite regardless
    pub step: Option<f64>,                 // inspector drag step
    pub unit: Option<Unit>,                // Meters, Seconds, Kilograms, Radians (shown as degrees), Degrees, ...
    pub flags: FieldFlags,                 // READ_ONLY, HIDDEN, TRANSIENT (runtime only: not saved), NO_LUA
    pub lua: Option<&'static str>,         // flat Luau name on Object, e.g. Material.color -> "color"
}

pub struct EnumInfo { pub name: &'static str, pub doc: &'static str, pub variants: &'static [VariantInfo] }
pub struct VariantInfo { pub name: &'static str, pub doc: &'static str, pub fields: &'static [FieldInfo] }

pub struct TypeInfo {
    pub name: &'static str,                // "Body"
    pub doc: &'static str,
    pub fields: &'static [FieldInfo],
}

pub trait Reflect: Sized + 'static {
    fn type_info() -> &'static TypeInfo;
    fn to_value(&self) -> Value;
    /// Whole-value decode with validation (type, range, finite). Missing fields take defaults.
    fn from_value(v: &Value) -> Result<Self, ReflectError>;
    /// Path access: "mass", "shape.radius".
    fn get(&self, path: &str) -> Result<Value, ReflectError>;
    fn set(&mut self, path: &str, v: Value) -> Result<(), ReflectError>;
}

/// Errors carry the type, the path, the reason and a did-you-mean list (field names of that type);
/// they map 1:1 to `SparkError` codes in spark-script / spark-devtools.
pub enum ReflectError {
    UnknownField { ty: &'static str, path: String, suggestions: Vec<&'static str> },
    WrongType { path: String, expected: &'static str, got: &'static str },
    OutOfRange { path: String, value: f64, min: f64, max: f64 },
    NotFinite { path: String },
    ReadOnly { path: String },
}
```

```rust
// spark-core: components of an object, registered explicitly
pub struct ComponentInfo {
    pub name: &'static str,                // "body"
    pub ty: &'static TypeInfo,
    pub required: bool,                    // transform: true
    pub get: fn(&Object) -> Option<Value>,                      // None = component absent
    pub set: fn(&mut Object, &str, Value) -> Result<(), ReflectError>, // "mass", value
    pub add: fn(&mut Object, Option<&Value>) -> Result<(), ReflectError>,
    pub remove: fn(&mut Object) -> bool,
}
pub struct Registry { pub components: &'static [ComponentInfo] }
pub fn registry() -> &'static Registry;   // spark_core::registry(), a plain static table
```

Usage (what the derive emits metadata from):

```rust
/// Rigid body simulated by physics.
#[derive(Clone, Debug, PartialEq, Reflect)]
pub struct Body {
    /// How the body moves.
    pub kind: BodyKind,
    /// Total mass; `None` = computed from volume (density 1).
    #[reflect(range = 0.001..=1.0e6, unit = "kg")]
    pub mass: Option<f32>,
    #[reflect(range = 0.0..=1.0, step = 0.01)]
    pub bounciness: f32,
    /// World-space velocity. Written back by the simulation.
    #[reflect(unit = "m/s", transient, lua = "velocity")]
    pub velocity: Vec3,
    // ...
}
```

Rules: documentation comes from `///` (a lint test fails for a registered field without doc, which is how "docs are complete" is enforced); defaults come from `Default::default()` unless `#[reflect(default = expr)]`; unit-variant enums use `#[derive(ReflectEnum)]` and reuse the existing `parse`/`name` pairs (`BodyKind`, `BlendMode`); data-bearing enums (`Shape`, `LightKind`) are `Value::Enum`. Float setters reject NaN/inf always, replacing the scattered hand-written finite checks. Range violation is an error by default (scripts keep today's behavior of explicit errors), and `#[reflect(clamp)]` is available for fields where the engine clamps today (`with_roughness`).

### How each consumer uses it

| Consumer | Uses |
|---|---|
| Luau bindings (`spark-script`) | At `Obj` userdata setup, iterate `registry()`; for every field with `lua = Some(name)` register `add_field_method_get/set` closures capturing `&'static FieldInfo` and the component accessor. Cost per access = what mlua already does + one `fn` pointer call (no string lookup at run time). Conversions `Value <-> Lua` are generic per `FieldType`; hand-written adapters stay for composite handles (`body`, `light`, `material`, `mesh`, `lod`, `shader`, `parent`). Unknown-field errors take their suggestion list from the registry + the (hand-written) method list. |
| Inspector (`spark-editor`) | Panels are built from `ComponentInfo`/`FieldInfo`: widget by `ty`, drag range from `range`/`step`, reset from `default`, tooltip from `doc`, disabled from `READ_ONLY`. A write is `Command::SetProperty` (ADR 0005). |
| Scene I/O (`spark-scene`) | `to_value`/`from_value` + `Value` <-> TOML; omit field when `== default()`; unknown key = error with suggestions; field order = declaration order. |
| Docs (`spark-api`) | `spark_reflect::luau_decl(&Registry) -> String` emits the `Object` field block and `SpawnProps`/`BodySpec`/`LightSpec` types with doc comments, and `api.json` gains a `components` section (name, doc, fields with type/range/default/unit). Spliced into `docs/spark.d.luau` between `-- @generated components begin/end` markers by the existing bless step. |
| Devtools / MCP | `SetProperty { node, path, value }` -> `ComponentInfo::set`; JSON <-> `Value` conversion lives in `spark-devtools` (it already owns `serde_json`). The registry dump (`api.json` `components`) is the MCP schema resource. |

### Crates and dependencies

- `spark-reflect`: no dependencies. `spark-reflect-derive`: `syn` (2.x, features `derive`, `parsing`, `proc-macro`, `printing`, `clone-impls`, the set `serde_derive` already compiles), `quote`, `proc-macro2`.
- `spark-core` -> `spark-reflect` (+ derive). `spark-core` gets no serde and no TOML. JSON/TOML conversion of `Value` lives in `spark-devtools` / `spark-scene`.
- Registry is a plain `static` table listed by hand in `spark-core/src/registry.rs`. A test destructures `Object` exhaustively (`let Object { name, .. } = ...`) so adding an `Object` field without a registry decision fails to compile.

### Relation to ADR 0001

ADR 0001 keeps `docs/spark.d.luau` as the hand-written source for **functions, globals, namespaces**. This ADR changes only the **component/property part**: the `Object` fields, `SpawnProps`, `BodySpec`, `LightSpec` become generated blocks inside the same file. The parser, `API_REFERENCE.md`, `llms.txt`, `api.json` and `spark api search` are unchanged; `api_drift.rs` keeps checking functions and gains an automatic check that every registry `lua` name exists in the VM. The `OBJ_MEMBERS` field names and `PROPS` strings are deleted; the method names stay a hand-written list (methods are not reflected: they are behavior, and mlua cannot enumerate them).

Implemented in step C-10: `spark-reflect::luau_decl` holds the registry-independent parts (`FieldType` to Luau type text, doc comments with range, unit and default); the walk over the registry (`spark-core`) is `spark-api/src/components.rs`, so `spark-reflect` still does not depend on `spark-core`. `docs/spark.d.luau` has three marker regions (spec types `BodySpec` / `LightSpec` / `SpawnProps`, the fields of `LightRef`, the registry fields of `Object`); `SPARK_BLESS=1 cargo test -p spark-api` rewrites them, and `api.json` gets a `components` section.

## Consequences

- One edit (a field + its attributes) updates Luau access, did-you-mean, inspector, files, devtools and docs; the drift tests for fields become redundant for the generated blocks.
- We own a derive: about 400-600 lines of macro code, compile-fail/UI tests for bad attributes, and the maintenance of our `Value` type. In return no dependency churn and error messages in the Spark style (codes, hints).
- Compile time: one proc-macro crate on top of `syn`/`quote` that `serde_derive` already pulls in; derive output is plain static data and small `match` bodies. Expected cost: seconds on a clean build, zero on incremental builds of unrelated crates. Measured at the end of step C-2 (Windows, rustc 1.99, dev profile with dependencies at opt-level 3): `cargo build -p spark-reflect` (both crates, `syn`/`quote`/`proc-macro2` already built) 2.5 s; from a fully empty target directory including `proc-macro2`, `quote` and `syn` 13.8 s (release: 11.3 s); incremental no-op 0.2 s. `syn` uses the extra `full` feature (needed to parse `a..=b` and arbitrary `default = expr`), which `serde_derive` does not enable; this is part of the 13.8 s. `trybuild` (dev-dependency of `spark-reflect-derive` only) adds `glob` and `target-triple` and is not part of any shipped binary, so `THIRD_PARTY_LICENSES.md` is unchanged.
- Implemented shapes differ from the sketch above in these points: `FieldType::Enum` / `FieldType::Struct` hold accessor functions (`fn() -> &'static EnumInfo` / `TypeInfo`) because a `static` initializer cannot call `Reflect::type_info()`; the derives also implement a helper trait `ReflectField` (type, `to_val`/`from_val`, sub-paths) so that field types are not guessed from syntax; `ReflectEnum` is a trait (`enum_info`, `variant_name`, `to_value`, `from_value`, `get`, `set`) and also covers data enums (`Shape`, `LightKind`) with named-field variants; `FieldFlags` has an extra `CLAMP` bit; `Unit` is a closed enum parsed from symbols (`m s kg rad deg m/s rad/s Hz %`); `color`, `asset = "..."`, `node_ref` field attributes retag `[f32; 4]` / `String`; `glam` vectors are not reflected directly (arrays `[f32; N]` are; a `glam` impl needs an optional feature or newtypes and is decided in step C-3).
- `FieldInfo` uses `&'static str` and `fn` pointers, so script `props` (dynamic names from Luau) are not in the static registry. They are exposed as `script.props.<name>` through `Value::Map` with an owned `DynField` list built from the module's `props` table.
- Luau field access stays on the fast path: no per-access name lookup. A micro-benchmark (reads and writes of `obj.position`, `obj.color` over 10k objects) is a gate for the generated-binding step.
- The reflection is deliberately not general: no trait objects over arbitrary user types, no reflection of methods, no dynamic type registration, no `Any`-based cloning. If a plugin ever needs custom components that is a new ADR.

## Alternatives rejected

- **`bevy_reflect`.** Mature, generic, handles enums, lists, maps, paths, serialization via serde. Rejected because: (1) it pulls a sizable dependency tree that is not in `Cargo.lock` today (`bevy_utils`, `bevy_ptr`, `bevy_platform`, `bevy_reflect_derive`, `erased-serde`, `downcast-rs`, `smallvec`, `uuid`, `thiserror`, ... on the order of 15-25 crates; not measured here) and a faster-moving MSRV than our `rust-version = 1.85` (not verified, must be checked before any reconsideration); (2) it describes Rust types, not UI/doc metadata: ranges, units, docs, did-you-mean and Luau names need its custom-attribute mechanism plus our own layer on top, so we would write `FieldInfo` anyway; (3) it implies serde on `spark-core` and trait-object (`dyn Reflect`) machinery we do not need for a closed component set; (4) breaking releases roughly every three months would make our scene API depend on Bevy's schedule.
- **Hand-written tables** (a `static FIELDS: &[...]` per component next to the struct). Zero dependencies, trivial to start, and what we effectively have today. Rejected because it is the drift problem again: a new field needs a manual table row, and only a test can notice a forgotten one. A derive makes the struct itself the table.
- **A declarative `macro_rules!` that defines struct and metadata together.** No proc-macro crate, but a private syntax for struct definitions, poor rust-analyzer/rustfmt support, bad error messages, and doc comments/attributes need awkward forwarding. Kept as a fallback if the derive's build cost turns out to be unacceptable.
- **Generating the registry from `docs/spark.d.luau`** (extend ADR 0001's direction). The `.d.luau` file is Luau-typed text and cannot hold ranges, defaults or units; the Rust types would still need hand-written glue. Rejected: the source of truth belongs next to the data.
- **`serde` + `schemars`-style JSON schema as the registry.** Gets serialization and a schema for free, but no widget hints, no `get/set` by path, and it makes serde a mandatory dependency of the core. Inspector and command paths would need a second mechanism anyway.

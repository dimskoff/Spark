//! The component part of the API, generated from the reflection registry (ADR 0001 amended by ADR 0004).
//!
//! `docs/spark.d.luau` stays the hand-written source of functions, globals and namespaces. Three regions of it sit
//! between `-- @generated components begin` and `-- @generated components end` and are written by [`splice`]:
//!
//! 1. the spec types `BodySpec`, `LightSpec`, `SpawnProps` (the tables `spawn`, `add_body`, `light` accept),
//! 2. the fields of `LightRef` (the `obj.light` handle),
//! 3. the flat fields of `Object` that are registry fields (`position`, `color`, `mass`, ...).
//!
//! Everything the registry cannot say stays here as a small table of hand-written entries (handles such as `parent` or
//! `body`, the `x/y/z` shortcuts live in the `.d.luau` itself): the types of handle-like fields and the spec keys
//! that are not registry fields.

use serde::Serialize;
use spark_core::registry::{LuaScope, registry};
use spark_reflect::{EnumInfo, FieldFlags, FieldInfo, FieldType, Flavor, TypeInfo, Unit, Value, field_doc, luau_type, optional};

pub const BEGIN: &str = "-- @generated components begin";
pub const END: &str = "-- @generated components end";

/// Registry enums that have a hand-written alias in `spark.d.luau`.
const ALIASES: &[(&str, &str)] = &[("BlendMode", "BlendMode"), ("BodyKind", "BodyType")];

/// Luau type of fields whose registry type is a runtime handle or a packed value: what a script reads.
const READ_TYPES: &[(&str, &str)] = &[
    ("data", "{ number }"),
    // `nil` writes remove the mass, but a body-less object reads as a number for typed scripts (as it always did).
    ("mass", "number"),
    ("mesh", "Mesh?"),
    ("lod", "{ LodLevel }"),
    ("shader", "Shader?"),
    ("normal_map", "Texture?"),
    ("orm_map", "Texture?"),
];

/// Luau type of spawn-table keys that accept more than the registry type says.
const SPEC_TYPES: &[(&str, &str)] = &[("scale", "(number | vector)?"), ("data", "{ number }?")];

/// Spawn-table keys that are not plain registry fields: (key, type, doc).
const SPAWN_EXTRAS: &[(&str, &str, &str)] = &[
    ("parent", "Object?", "Parent object; position, rotation and scale are then relative to it."),
    ("body", "(BodyType | boolean | BodySpec)?", "Adds a rigid body: a type name, `true` (dynamic) or a spec table."),
    ("light", "LightSpec?", "Adds a point or spot light."),
    ("shader", "Shader?", "Custom surface shader."),
    ("normal_map", "(Texture | string | false)?", "Normal map: a texture or an image path; `false` removes it."),
    ("orm_map", "(Texture | string | false)?", "Occlusion / roughness / metallic map: a texture or an image path; `false` removes it."),
    ("node", "string?", "spawn(model, { node = \"name\" }): only that node of a model (and its children), moved to the origin in X / Z"),
    ("lod", "{ LodLevel }?", "Level-of-detail meshes: `{ distance, mesh }` pairs."),
];

/// Spec keys of a light that are not registry fields.
const LIGHT_EXTRAS: &[(&str, &str, &str)] = &[
    ("position", "vector?", "Position of the light object."),
    ("rotation", "vector?", "Rotation of the light object (spot lights shine along -Z)."),
    ("parent", "Object?", "Parent object."),
    ("name", "string?", "Name of the light object."),
];

/// Components whose fields are not spawn-table keys: a `body` is added with `body = { ... }`.
const NOT_SPAWN_KEYS: &[&str] = &["body"];

/// Docs of fields that scripts see differently from Rust (radians, `nil`): by Luau name, replacing the `///` text.
const LUA_DOCS: &[(&str, &str)] = &[
    (
        "rotation",
        "Rotation as Euler angles in radians: (pitch, yaw, roll) = rotation around X, Y, Z, applied Y, then X, then Z. Relative to the parent.",
    ),
    ("angle", "Full cone angle of a spot light in degrees, 1 to 179 (new spot lights: 45)."),
    ("mass", "Total mass in kg. Assigning `nil` lets the physics engine compute it from the volume (density 1)."),
    ("shader", "Custom surface shader (`nil` = the engine default)."),
];

/// Registry names that are written differently in a spec table: `kind` -> `type`, `linear_damping` -> `damping`.
const BODY_SPEC_NAMES: &[(&str, &str)] = &[("kind", "type"), ("linear_damping", "damping")];

fn doc_of(f: &FieldInfo, with_default: bool) -> Vec<String> {
    field_doc(f, f.lua.and_then(|l| find(LUA_DOCS, l)), with_default)
}

fn find<'a>(table: &'a [(&str, &str)], key: &str) -> Option<&'a str> {
    table.iter().find(|(k, _)| *k == key).map(|(_, v)| *v)
}

struct Entry {
    name: String,
    ty: String,
    doc: Vec<String>,
}

impl Entry {
    fn render(&self, indent: &str, sep: &str) -> String {
        let mut s = String::new();
        for d in &self.doc {
            s.push_str(&format!("{indent}-- {d}\n"));
        }
        s.push_str(&format!("{indent}{}: {}{sep}\n", self.name, self.ty));
        s
    }
}

/// Fields of a component with a Luau name, flat on the object (node fields first, then the components whose Luau
/// scope is `Object`, in registry order).
fn object_fields() -> Vec<&'static FieldInfo> {
    let reg = registry();
    let mut out: Vec<&'static FieldInfo> = reg.node_info().fields.iter().filter(|f| f.lua.is_some()).collect();
    for c in reg.components.iter().filter(|c| c.lua == LuaScope::Object) {
        out.extend(c.info().fields.iter().filter(|f| f.lua.is_some()));
    }
    out
}

/// A field with a Luau name, as members of a class or a spec table.
fn entry(f: &FieldInfo, name: &str, flavor: Flavor, doc: Vec<String>) -> Entry {
    let lua = f.lua.unwrap_or(f.name);
    let ty = match flavor {
        Flavor::Read => find(READ_TYPES, lua).map(str::to_string).unwrap_or_else(|| luau_type(&f.ty, flavor, ALIASES)),
        Flavor::Spec => match find(SPEC_TYPES, lua) {
            Some(t) => t.to_string(),
            None => optional(&luau_type(&f.ty, flavor, ALIASES)),
        },
    };
    Entry { name: name.to_string(), ty, doc }
}

/// The fields of the variants of an enum field (`Shape::Box { size }`, `LightKind::Spot { angle, softness }`), one
/// entry per distinct name, with a note about which variants use it.
fn variant_fields(info: &'static EnumInfo) -> Vec<(&'static FieldInfo, String)> {
    let mut out: Vec<(&'static FieldInfo, Vec<&'static str>)> = Vec::new();
    for v in info.variants {
        for f in v.fields {
            match out.iter_mut().find(|(g, _)| g.name == f.name) {
                Some((_, used)) => used.push(v.name),
                None => out.push((f, vec![v.name])),
            }
        }
    }
    out.into_iter()
        .map(|(f, used)| (f, format!("Used by {}.", used.iter().map(|u| format!("\"{u}\"")).collect::<Vec<_>>().join(", "))))
        .collect()
}

fn enum_of(f: &FieldInfo) -> Option<&'static EnumInfo> {
    match f.ty {
        FieldType::Enum(e) => Some(e()),
        _ => None,
    }
}

/// `type Name = { ... }`
fn spec_type(name: &str, entries: &[Entry]) -> String {
    let mut s = format!("type {name} = {{\n");
    for e in entries {
        s.push_str(&e.render("    ", ","));
    }
    s.push_str("}\n");
    s
}

fn body_spec() -> Vec<Entry> {
    let info = registry().component("body").expect("body component").info();
    let mut out = Vec::new();
    for f in info.fields.iter().filter(|f| !f.flags.contains(FieldFlags::HIDDEN)) {
        let name = find(BODY_SPEC_NAMES, f.name).unwrap_or(f.name);
        // The body's shape is `shape = "box"` plus the size keys of the variants.
        out.push(entry(f, name, Flavor::Spec, doc_of(f, true)));
        if let Some(e) = enum_of(f).filter(|e| e.variants.iter().any(|v| !v.fields.is_empty())) {
            for (vf, used) in variant_fields(e) {
                let mut doc = doc_of(vf, true);
                doc.push(used);
                out.push(entry(vf, vf.name, Flavor::Spec, doc));
            }
        }
    }
    out
}

fn light_entries(flavor: Flavor) -> Vec<Entry> {
    let info = registry().component("light").expect("light component").info();
    let mut out = Vec::new();
    for f in info.fields.iter().filter(|f| f.lua.is_some()) {
        let doc = doc_of(f, flavor == Flavor::Spec);
        out.push(entry(f, f.lua.unwrap_or(f.name), flavor, doc));
    }
    for f in info.fields {
        if let Some(e) = enum_of(f).filter(|e| e.variants.iter().any(|v| !v.fields.is_empty())) {
            for (vf, used) in variant_fields(e) {
                let mut doc = doc_of(vf, flavor == Flavor::Spec);
                doc.push(used);
                out.push(entry(vf, vf.lua.unwrap_or(vf.name), flavor, doc));
            }
        }
    }
    out
}

fn spawn_props() -> Vec<Entry> {
    let reg = registry();
    let mut out = Vec::new();
    let mut fields: Vec<&'static FieldInfo> = reg.node_info().fields.iter().filter(|f| f.lua.is_some()).collect();
    for c in reg.components.iter().filter(|c| c.lua == LuaScope::Object && !NOT_SPAWN_KEYS.contains(&c.name)) {
        fields.extend(c.info().fields.iter().filter(|f| f.lua.is_some()));
    }
    for f in fields.into_iter().filter(|f| !f.flags.contains(FieldFlags::READ_ONLY)) {
        out.push(entry(f, f.lua.unwrap_or(f.name), Flavor::Spec, doc_of(f, true)));
    }
    for (name, ty, doc) in SPAWN_EXTRAS {
        out.push(Entry { name: name.to_string(), ty: ty.to_string(), doc: vec![doc.to_string()] });
    }
    out
}

fn spec_types_block() -> String {
    let mut light = light_entries(Flavor::Spec);
    for (name, ty, doc) in LIGHT_EXTRAS {
        light.push(Entry { name: name.to_string(), ty: ty.to_string(), doc: vec![doc.to_string()] });
    }
    // BodySpec, LightSpec and SpawnProps are separated by single blank lines.
    [spec_type("BodySpec", &body_spec()), spec_type("LightSpec", &light), spec_type("SpawnProps", &spawn_props())].join("\n")
}

fn light_ref_block() -> String {
    light_entries(Flavor::Read).iter().map(|e| e.render("    ", "")).collect()
}

fn object_block() -> String {
    let mut s = String::new();
    for f in object_fields() {
        let lua = f.lua.unwrap_or(f.name);
        s.push_str(&entry(f, lua, Flavor::Read, doc_of(f, true)).render("    ", ""));
    }
    s
}

/// The generated regions of `spark.d.luau`, in the order they appear in the file.
pub fn blocks() -> Vec<String> {
    vec![spec_types_block(), light_ref_block(), object_block()]
}

/// Replaces the text between each `begin` / `end` marker pair with the generated block. Errors when the markers are
/// missing, unbalanced or not exactly as many as there are blocks.
pub fn splice(src: &str) -> Result<String, String> {
    let blocks = blocks();
    let mut out = String::new();
    let mut lines = src.lines();
    let mut used = 0;
    while let Some(line) = lines.next() {
        out.push_str(line);
        out.push('\n');
        if line.trim() == BEGIN {
            let Some(block) = blocks.get(used) else { return Err(format!("more than {} `{BEGIN}` markers", blocks.len())) };
            used += 1;
            out.push_str(block);
            loop {
                match lines.next() {
                    Some(l) if l.trim() == END => {
                        out.push_str(l);
                        out.push('\n');
                        break;
                    }
                    Some(_) => {}
                    None => return Err(format!("`{BEGIN}` without `{END}`")),
                }
            }
        }
    }
    if used != blocks.len() {
        return Err(format!("expected {} `{BEGIN}` markers, found {used}", blocks.len()));
    }
    Ok(out)
}

// ---------------------------------------------------------------------------------------------------------
// api.json: the `components` section

/// One field of a component, for tools (MCP schema, editors).
#[derive(Debug, Serialize)]
pub struct FieldJson {
    pub name: String,
    /// Name on the Luau object (flat, or a member of the handle named by the component's `lua_scope`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lua: Option<String>,
    /// `bool`, `int`, `float`, `string`, `vec2`, `vec3`, `vec4`, `color`, `enum`, `struct`, `option`, `list`, `node_ref`, `asset`.
    pub kind: String,
    /// Type as Luau scripts read it, when the registry knows one.
    pub luau_type: String,
    pub doc: String,
    pub default: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub range: Option<RangeJson>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub step: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    pub read_only: bool,
    pub hidden: bool,
    pub transient: bool,
    /// Enum values (`"dynamic"`, ...) with their own fields.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub variants: Vec<VariantJson>,
}

#[derive(Debug, Serialize)]
pub struct RangeJson {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min: Option<f64>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub min_exclusive: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
    pub clamp: bool,
}

#[derive(Debug, Serialize)]
pub struct VariantJson {
    pub name: String,
    pub doc: String,
    pub fields: Vec<FieldJson>,
}

#[derive(Debug, Serialize)]
pub struct ComponentJson {
    pub name: String,
    pub doc: String,
    /// Always present on an object (`transform`, `material`).
    pub required: bool,
    /// `"object"`: the Luau names are flat on the object (`obj.position`); `"handle:light"`: members of `obj.light`.
    pub lua_scope: String,
    pub fields: Vec<FieldJson>,
}

#[derive(Debug, Serialize)]
pub struct ComponentsJson {
    /// The node-level fields (`name`, `visible`).
    pub node: ComponentJson,
    pub components: Vec<ComponentJson>,
}

fn json_number(x: f64) -> serde_json::Value {
    // f32 values widened to f64 print as 0.30000001192...: go through the shortest f32 text.
    let f = x as f32;
    let n = if f.is_finite() && f64::from(f) == x { format!("{f}").parse::<f64>().unwrap_or(x) } else { x };
    serde_json::Number::from_f64(n).map_or(serde_json::Value::Null, serde_json::Value::Number)
}

fn value_json(v: &Value) -> serde_json::Value {
    use serde_json::{Value as J, json};
    let arr = |c: &[f32]| J::Array(c.iter().map(|x| json_number(f64::from(*x))).collect());
    match v {
        Value::Bool(b) => json!(b),
        Value::Int(i) => json!(i),
        Value::Float(f) => json_number(*f),
        Value::Str(s) | Value::Asset(s) | Value::NodeRef(s) => json!(s),
        Value::Vec2(c) => arr(c),
        Value::Vec3(c) => arr(c),
        Value::Vec4(c) | Value::Color(c) => arr(c),
        Value::Enum { variant, fields } if fields.is_empty() => json!(variant),
        Value::Enum { variant, fields } => {
            let mut m = serde_json::Map::new();
            m.insert("variant".into(), json!(variant));
            for (k, v) in fields {
                m.insert(k.clone(), value_json(v));
            }
            J::Object(m)
        }
        Value::List(items) => J::Array(items.iter().map(value_json).collect()),
        Value::Map(pairs) => J::Object(pairs.iter().map(|(k, v)| (k.clone(), value_json(v))).collect()),
        Value::None => J::Null,
    }
}

fn field_json(f: &FieldInfo, scope_read: bool) -> FieldJson {
    let lua = f.lua.map(str::to_string);
    let luau = match f.lua.and_then(|l| find(READ_TYPES, l)) {
        Some(t) if scope_read => t.to_string(),
        _ => luau_type(&f.ty, Flavor::Read, ALIASES),
    };
    let range = f.range.map(|(min, max)| RangeJson {
        min: Some(if min > 0.0 && min < 1.0e-30 { 0.0 } else { min }),
        min_exclusive: min > 0.0 && min < 1.0e-30,
        max: (max < f64::from(f32::MAX) * 0.99).then_some(max),
        clamp: f.flags.contains(FieldFlags::CLAMP),
    });
    let range = range.map(|r| RangeJson { min: r.min.map(json_f), max: r.max.map(json_f), ..r });
    let variants = match f.ty {
        FieldType::Enum(e) => e().variants.iter().map(variant_json).collect(),
        _ => Vec::new(),
    };
    FieldJson {
        name: f.name.to_string(),
        lua,
        kind: f.ty.name().to_string(),
        luau_type: luau,
        doc: f.doc.trim().to_string(),
        default: value_json(&(f.default)()),
        range,
        step: f.step.map(json_f),
        unit: f.unit.map(|u: Unit| u.symbol().to_string()),
        read_only: f.flags.contains(FieldFlags::READ_ONLY),
        hidden: f.flags.contains(FieldFlags::HIDDEN),
        transient: f.flags.contains(FieldFlags::TRANSIENT),
        variants,
    }
}

/// An f32 limit widened to f64, printed as the f32 would be.
fn json_f(x: f64) -> f64 {
    format!("{}", x as f32).parse::<f64>().unwrap_or(x)
}

fn variant_json(v: &spark_reflect::VariantInfo) -> VariantJson {
    VariantJson { name: v.name.to_string(), doc: v.doc.trim().to_string(), fields: v.fields.iter().map(|f| field_json(f, false)).collect() }
}

fn type_json(name: &str, info: &TypeInfo, required: bool, scope: &str, read_types: bool) -> ComponentJson {
    ComponentJson {
        name: name.to_string(),
        doc: info.doc.trim().to_string(),
        required,
        lua_scope: scope.to_string(),
        fields: info.fields.iter().map(|f| field_json(f, read_types)).collect(),
    }
}

/// The `components` section of `docs/api.json`.
pub fn components_json() -> ComponentsJson {
    let reg = registry();
    ComponentsJson {
        node: type_json("node", reg.node_info(), true, "object", true),
        components: reg
            .components
            .iter()
            .map(|c| {
                let scope = match c.lua {
                    LuaScope::Object => "object".to_string(),
                    LuaScope::Handle(h) => format!("handle:{h}"),
                    LuaScope::None => "none".to_string(),
                };
                type_json(c.name, c.info(), c.required, &scope, c.lua == LuaScope::Object)
            })
            .collect(),
    }
}

/// `docs/api.json`: the parsed API plus the registry's components.
#[derive(Serialize)]
struct ApiJson<'a> {
    #[serde(flatten)]
    api: &'a crate::Api,
    components: ComponentsJson,
}

/// Pretty JSON of the whole API with the `components` section (what `docs/api.json` holds).
pub fn api_json(api: &crate::Api) -> String {
    serde_json::to_string_pretty(&ApiJson { api, components: components_json() }).expect("api serializes") + "\n"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shipped_file_is_spliced() {
        let spliced = splice(crate::D_LUAU).expect("markers in docs/spark.d.luau");
        // The shipped file is current (the bless test in tests/generated.rs writes it): splicing is idempotent.
        assert_eq!(splice(&spliced).unwrap(), spliced);
    }

    #[test]
    fn splice_needs_the_markers() {
        assert!(splice("type X = number\n").is_err());
        assert!(splice(&format!("{BEGIN}\n")).is_err());
    }

    #[test]
    fn json_numbers_are_short() {
        assert_eq!(json_number(f64::from(0.3f32)), serde_json::json!(0.3));
    }
}

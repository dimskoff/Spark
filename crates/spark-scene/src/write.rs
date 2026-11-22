//! [`SceneFile`] -> canonical TOML text.
//!
//! One document has exactly one text (ADR 0003, rule 9), so `load -> save` of a canonical file is byte-identical and a
//! change of one property is a change of one line. The rules are listed in `docs/SCENE_FORMAT.md`; in short:
//!
//! - UTF-8, LF, one blank line before every table, one final newline, TOML 1.0 constructs only;
//! - a leading comment block, `[scene]` / `[prefab]`, `[environment...]`, then one `[[node]]` per node in the
//!   document's order, each followed by its component tables in canonical order;
//! - node keys: `id`, `parent`, `name`, `visible` (only when false), `prefab`, `tags`;
//! - fields in registry declaration order; fields equal to their default are omitted (optional components that are
//!   present are written as an empty table); runtime-only fields never;
//! - numbers: shortest text that reads back as the same `f32`; float scalars always have a decimal point
//!   (`1.0`, `12.0`), vector components do not (`[0, 3, 8]`); `-0` is `0`; angles in degrees (4 decimals at most for
//!   rotations);
//! - colors `"#rrggbb"` / `"#rrggbbaa"` when exactly representable, else `[r, g, b, a]`;
//! - enums: unit variants as strings, variants with data as `{ type = "spot", angle = 50.0 }`;
//! - mesh recipes always list their parameters (`primitive = "box"`, `size = [1, 1, 1]`).

use spark_core::MeshSource;
use spark_core::doc::{
    Environment, FileKind, FileNode, RESERVED_COMPONENTS, SceneFile, component_rank, mesh_from_value, normalize_component, round_degrees,
};
use spark_core::registry::registry;
use spark_core::{Fog, Sky, Sun};
use spark_reflect::{EnumInfo, FieldFlags, FieldInfo, FieldType, Reflect, ReflectEnum, TypeInfo, Unit, Value};

use crate::error::{SaveError, SaveIssue};
use crate::migrate::FORMAT_VERSION;
use crate::prefab::{Target, override_target, split_override_key};
use crate::text::{array, color_text, f32_scalar, f64_scalar, key, quote, radians_to_degrees};

/// The canonical text of `file`. Fails, listing every problem, when the document has parts that cannot be saved:
/// runtime-only meshes (`Mesh.custom`), texture / shader handles, invalid values.
pub fn to_string(file: &SceneFile) -> Result<String, SaveError> {
    let (text, issues) = to_string_lossy(file);
    if issues.is_empty() { Ok(text) } else { Err(SaveError { issues }) }
}

/// The canonical text of what can be saved, and a list of what was left out (never silent).
pub fn to_string_lossy(file: &SceneFile) -> (String, Vec<SaveIssue>) {
    let mut w = Writer { out: String::new(), tables: 0, issues: Vec::new() };
    w.file(file);
    (w.out, w.issues)
}

struct Writer {
    out: String,
    tables: usize,
    issues: Vec<SaveIssue>,
}

impl Writer {
    /// A table (or `[[array]]` element) with its lines; tables are separated by one blank line.
    fn table(&mut self, header: &str, lines: &[String]) {
        if self.tables > 0 {
            self.out.push('\n');
        }
        self.tables += 1;
        self.out.push_str(header);
        self.out.push('\n');
        for l in lines {
            self.out.push_str(l);
            self.out.push('\n');
        }
    }

    fn issue(&mut self, node: Option<&FileNode>, component: &str, message: impl Into<String>) {
        self.issues.push(SaveIssue {
            node: node.map(|n| n.id.clone()),
            node_name: node.map(|n| n.name.clone()).unwrap_or_default(),
            component: component.to_string(),
            message: message.into(),
        });
    }

    fn file(&mut self, file: &SceneFile) {
        for l in &file.comment {
            self.out.push_str(l);
            self.out.push('\n');
        }
        let mut header = vec![format!("format_version = {FORMAT_VERSION}")];
        if !file.name.is_empty() {
            header.push(format!("name = {}", quote(&file.name)));
        }
        self.table(&format!("[{}]", file.kind.header()), &header);
        if file.kind == FileKind::Scene {
            self.environment(&file.environment);
        }
        for n in &file.nodes {
            self.node(n);
        }
    }

    fn environment(&mut self, env: &Environment) {
        let def = Environment::default();
        let mut lines = Vec::new();
        let color = |c: spark_core::Color| color_text([c.r, c.g, c.b, c.a]);
        if env.ambient != def.ambient {
            lines.push(format!("ambient = {}", color(env.ambient)));
        }
        if env.background != def.background {
            lines.push(format!("background = {}", color(env.background)));
        }
        if !lines.is_empty() {
            self.table("[environment]", &lines);
        }
        let sun = struct_lines(Sun::type_info().fields, &pairs(env.sun.to_value()));
        if !sun.is_empty() {
            self.table("[environment.sun]", &sun);
        }
        if let Some(fog) = &env.fog {
            self.table("[environment.fog]", &struct_lines(Fog::type_info().fields, &pairs(fog.to_value())));
        }
        if let Some(sky) = &env.sky {
            if sky.cubemap.is_some() {
                self.issue(
                    None,
                    "environment.sky",
                    "`cubemap` is a runtime handle (no scene-file form until assets have paths); it was left out",
                );
            }
            self.table("[environment.sky]", &struct_lines(Sky::type_info().fields, &pairs(sky.to_value())));
        }
    }

    fn node(&mut self, n: &FileNode) {
        let mut lines = vec![format!("id = {}", quote(n.id.as_str()))];
        if let Some(p) = &n.parent {
            match &n.parent_in {
                Some(inner) => lines.push(format!("parent = {}", quote(&format!("{p}/{inner}")))),
                None => lines.push(format!("parent = {}", quote(p.as_str()))),
            }
        }
        if !n.name.is_empty() {
            lines.push(format!("name = {}", quote(&n.name)));
        }
        if !n.visible {
            lines.push("visible = false".into());
        }
        if let Some(p) = &n.prefab {
            lines.push(format!("prefab = {}", quote(p)));
        }
        if !n.tags.is_empty() {
            let tags: Vec<String> = n.tags.iter().map(|t| quote(t)).collect();
            lines.push(format!("tags = [{}]", tags.join(", ")));
        }
        self.table("[[node]]", &lines);
        let mut comps: Vec<&(String, Value)> = n.components.iter().collect();
        comps.sort_by_key(|(name, _)| component_rank(name));
        for (name, v) in comps {
            self.component(n, name, v);
        }
        if !n.overrides.is_empty() {
            let mut lines = Vec::new();
            for (k, v) in n.overrides.iter().filter(|(_, v)| *v != Value::None) {
                match override_text(k, v) {
                    Ok(text) => lines.push(format!("{} = {text}", key(k))),
                    Err(message) => self.issue(Some(n), "overrides", format!("`{k}`: {message}; it was left out")),
                }
            }
            self.table("[node.overrides]", &lines);
        }
    }

    fn component(&mut self, n: &FileNode, name: &str, v: &Value) {
        let Some(info) = registry().component(name) else {
            if RESERVED_COMPONENTS.contains(&name) {
                match v {
                    Value::Map(p) => self.generic_table(&format!("node.{name}"), p),
                    _ => self.issue(Some(n), name, "expected a map of values"),
                }
            } else {
                self.issue(Some(n), name, "not a component (see the list in docs/SCENE_FORMAT.md)");
            }
            return;
        };
        if name == "mesh" {
            return self.mesh(n, v);
        }
        // Runtime handles (textures, shaders) have no file form: say so instead of dropping them silently.
        let mut v = v.clone();
        if let Value::Map(p) = &mut v {
            let ty = info.info();
            let mut gone = Vec::new();
            p.retain(|(k, val)| {
                let transient = ty.field(k).is_some_and(|f| f.flags.contains(FieldFlags::TRANSIENT));
                if transient && matches!(val, Value::Asset(_)) {
                    gone.push(k.clone());
                }
                !transient
            });
            for k in gone {
                self.issue(
                    Some(n),
                    name,
                    format!("`{k}` is a runtime handle (no scene-file form until assets have paths); it was left out"),
                );
            }
        }
        let v = match normalize_component(name, &v) {
            Ok(v) => v,
            Err(e) => return self.issue(Some(n), name, e.to_string()),
        };
        let lines = struct_lines(info.info().fields, &pairs(v));
        // A prefab instance lists the components it replaces, so one at its defaults is still written.
        if lines.is_empty() && info.required && n.prefab.is_none() {
            return;
        }
        self.table(&format!("[node.{name}]"), &lines);
    }

    fn mesh(&mut self, n: &FileNode, v: &Value) {
        let (source, lod) = match mesh_from_value(v) {
            Ok(x) => x,
            Err(e) => return self.issue(Some(n), "mesh", e.to_string()),
        };
        if source == MeshSource::RuntimeOnly {
            return self.issue(
                Some(n),
                "mesh",
                "the mesh was made by code (Mesh.custom or an edited mesh) and has no recipe; use a primitive or a model, or the node cannot be saved with its mesh",
            );
        }
        let mut lines = recipe_lines(&source);
        let mut levels = Vec::new();
        for (distance, s) in &lod {
            if *s == MeshSource::RuntimeOnly {
                self.issue(
                    Some(n),
                    "mesh",
                    format!("the level of detail at {distance} m was made by code and has no recipe; it was left out"),
                );
                continue;
            }
            let mut parts = vec![format!("distance = {}", f32_scalar(*distance))];
            parts.extend(recipe_lines(s));
            levels.push(format!("{{ {} }}", parts.join(", ")));
        }
        if !levels.is_empty() {
            lines.push(format!("lod = [{}]", levels.join(", ")));
        }
        self.table("[node.mesh]", &lines);
    }

    /// Free-form data: plain values first, then each non-empty nested map as its own `[path.key]` table.
    fn generic_table(&mut self, path: &str, p: &[(String, Value)]) {
        let nested = |v: &Value| matches!(v, Value::Map(m) if !m.is_empty());
        let lines: Vec<String> =
            p.iter().filter(|(_, v)| !nested(v) && *v != Value::None).map(|(k, v)| format!("{} = {}", key(k), generic(v))).collect();
        self.table(&format!("[{path}]"), &lines);
        for (k, v) in p {
            if let Value::Map(m) = v {
                if nested(v) {
                    self.generic_table(&format!("{path}.{}", key(k)), m);
                }
            }
        }
    }
}

fn pairs(v: Value) -> Vec<(String, Value)> {
    match v {
        Value::Map(p) => p,
        _ => Vec::new(),
    }
}

// ---------------------------------------------------------------------------------------------------------
// Typed values (driven by the registry's field metadata)

/// `name = value` for every saveable field that is not at its default, in declaration order.
fn struct_lines(fields: &[FieldInfo], p: &[(String, Value)]) -> Vec<String> {
    fields.iter().filter_map(|f| field_text(f, p).map(|t| format!("{} = {t}", key(f.name)))).collect()
}

/// The text of one field's value, `None` when it is not written (missing, runtime-only, default).
fn field_text(f: &FieldInfo, p: &[(String, Value)]) -> Option<String> {
    if f.flags.contains(FieldFlags::TRANSIENT) {
        return None;
    }
    let v = &p.iter().find(|(k, _)| k == f.name)?.1;
    if canon(f.unit, v) == canon(f.unit, &(f.default)()) {
        return None;
    }
    encode(&f.ty, f.unit, v)
}

/// Rotations are compared and written rounded (see `round_degrees`).
fn canon(unit: Option<Unit>, v: &Value) -> Value {
    if unit != Some(Unit::Degrees) {
        return v.clone();
    }
    let r = |x: f32| round_degrees(x);
    match v {
        Value::Float(x) => Value::Float(f64::from(r(*x as f32))),
        Value::Vec2(a) => Value::Vec2(a.map(r)),
        Value::Vec3(a) => Value::Vec3(a.map(r)),
        Value::Vec4(a) => Value::Vec4(a.map(r)),
        other => other.clone(),
    }
}

fn encode(ty: &FieldType, unit: Option<Unit>, v: &Value) -> Option<String> {
    let v = canon(unit, v);
    Some(match (ty, &v) {
        (_, Value::None) => return None,
        (FieldType::Option(inner), _) => return encode(inner, unit, &v),
        (_, Value::Bool(b)) => b.to_string(),
        (_, Value::Int(i)) if !matches!(ty, FieldType::Float) => i.to_string(),
        (_, Value::Float(_) | Value::Int(_)) => {
            let x = match &v {
                Value::Float(x) => *x as f32,
                Value::Int(i) => *i as f32,
                _ => unreachable!(),
            };
            f32_scalar(if unit == Some(Unit::Radians) { radians_to_degrees(x) } else { x })
        }
        (_, Value::Str(s) | Value::NodeRef(s) | Value::Asset(s)) => quote(s),
        (_, Value::Vec2(a)) => array(a),
        (_, Value::Vec3(a)) => array(a),
        (_, Value::Vec4(a)) => array(a),
        (_, Value::Color(a)) => color_text(*a),
        (FieldType::Enum(e), Value::Enum { variant, fields }) => enum_text(e(), variant, fields),
        (FieldType::Struct(t), Value::Map(p)) => inline_struct(t(), p),
        (FieldType::List(inner), Value::List(items)) => {
            let parts: Vec<String> = items.iter().filter_map(|i| encode(inner, unit, i)).collect();
            format!("[{}]", parts.join(", "))
        }
        (_, other) => generic(other),
    })
}

fn enum_text(e: &'static EnumInfo, variant: &str, fields: &[(String, Value)]) -> String {
    let Some(vi) = e.variant(variant).filter(|v| !v.fields.is_empty()) else { return quote(variant) };
    let mut parts = vec![format!("type = {}", quote(variant))];
    parts.extend(struct_lines(vi.fields, fields));
    format!("{{ {} }}", parts.join(", "))
}

fn inline_struct(t: &'static TypeInfo, p: &[(String, Value)]) -> String {
    let parts = struct_lines(t.fields, p);
    if parts.is_empty() { "{}".into() } else { format!("{{ {} }}", parts.join(", ")) }
}

/// The text of one prefab override: the field's value in its file form, or the whole component as an inline table.
/// An override is written even when it equals the field's default (the prefab's value may differ).
fn override_text(k: &str, v: &Value) -> Result<String, String> {
    let ok = split_override_key(k)?;
    let fields_of = |t: &'static TypeInfo| t.fields;
    match (override_target(&ok), ok.field) {
        (Target::Unknown, _) => Err(format!("`{}` is not a component", ok.component)),
        (Target::Node, Some(f)) => field_override(fields_of(registry().node_info()), f, v),
        (Target::Node, None) => Err("`node` has the fields `name` and `visible`".into()),
        (Target::Reserved, None) => Ok(generic(v)),
        (Target::Reserved, Some(_)) => Err(format!("`{}` is overridden whole", ok.component)),
        (Target::Registry, None) if ok.component == "mesh" => {
            let (source, lod) = mesh_from_value(v).map_err(|e| e.to_string())?;
            if source == MeshSource::RuntimeOnly {
                return Err("the mesh was made by code and has no recipe".into());
            }
            let mut parts = recipe_lines(&source);
            let levels: Vec<String> = lod
                .iter()
                .filter(|(_, s)| *s != MeshSource::RuntimeOnly)
                .map(|(d, s)| {
                    let mut p = vec![format!("distance = {}", f32_scalar(*d))];
                    p.extend(recipe_lines(s));
                    format!("{{ {} }}", p.join(", "))
                })
                .collect();
            if !levels.is_empty() {
                parts.push(format!("lod = [{}]", levels.join(", ")));
            }
            Ok(format!("{{ {} }}", parts.join(", ")))
        }
        (Target::Registry, Some(_)) if ok.component == "mesh" => Err("a mesh is overridden whole".into()),
        (Target::Registry, Some(f)) => {
            let info = registry().component(ok.component).expect("checked by override_target");
            field_override(info.info().fields, f, v)
        }
        (Target::Registry, None) => {
            let info = registry().component(ok.component).expect("checked by override_target");
            let v = normalize_component(ok.component, v).map_err(|e| e.to_string())?;
            let parts = struct_lines(info.info().fields, &pairs(v));
            Ok(if parts.is_empty() { "{}".into() } else { format!("{{ {} }}", parts.join(", ")) })
        }
    }
}

fn field_override(fields: &'static [FieldInfo], name: &str, v: &Value) -> Result<String, String> {
    let f = fields.iter().find(|f| f.name == name).ok_or_else(|| format!("no field `{name}`"))?;
    if f.flags.contains(FieldFlags::TRANSIENT) {
        return Err(format!("`{name}` is runtime state"));
    }
    encode(&f.ty, f.unit, v).ok_or_else(|| format!("no value for `{name}`"))
}

/// The keys of a mesh recipe: `primitive = "box"` + parameters, or `asset`, `node`, `part`.
fn recipe_lines(source: &MeshSource) -> Vec<String> {
    let Value::Enum { variant, fields } = source.to_value() else { return Vec::new() };
    let Some(vi) = MeshSource::enum_info().variant(&variant) else { return Vec::new() };
    let mut lines = Vec::new();
    if variant != "asset" {
        lines.push(format!("primitive = {}", quote(&variant)));
    }
    for f in vi.fields {
        let Some((_, v)) = fields.iter().find(|(k, _)| k == f.name) else { continue };
        // A recipe lists its parameters (defaults included), except flags that are off and the model's first primitive.
        let skip = match (f.name, v) {
            ("surface", Value::Bool(false)) => true,
            ("primitive", Value::Int(0)) => variant == "asset",
            _ => false,
        };
        if skip || *v == Value::None {
            continue;
        }
        let name = if variant == "asset" && f.name == "primitive" { "part" } else { f.name };
        if let Some(t) = encode(&f.ty, f.unit, v) {
            lines.push(format!("{} = {t}", key(name)));
        }
    }
    lines
}

/// Free-form values: TOML types as they are (an `Int` stays an integer, a `Float` a float).
fn generic(v: &Value) -> String {
    match v {
        Value::Bool(b) => b.to_string(),
        Value::Int(i) => i.to_string(),
        Value::Float(x) => f64_scalar(*x),
        Value::Str(s) | Value::NodeRef(s) | Value::Asset(s) => quote(s),
        Value::Vec2(a) => array(a),
        Value::Vec3(a) => array(a),
        Value::Vec4(a) => array(a),
        Value::Color(a) => color_text(*a),
        Value::Enum { variant, fields } if fields.is_empty() => quote(variant),
        Value::Enum { variant, fields } => {
            let mut parts = vec![format!("type = {}", quote(variant))];
            parts.extend(fields.iter().filter(|(_, v)| *v != Value::None).map(|(k, v)| format!("{} = {}", key(k), generic(v))));
            format!("{{ {} }}", parts.join(", "))
        }
        Value::List(items) => format!("[{}]", items.iter().filter(|i| **i != Value::None).map(generic).collect::<Vec<_>>().join(", ")),
        Value::Map(p) if p.is_empty() => "{}".into(),
        Value::Map(p) => {
            let parts: Vec<String> =
                p.iter().filter(|(_, v)| *v != Value::None).map(|(k, v)| format!("{} = {}", key(k), generic(v))).collect();
            format!("{{ {} }}", parts.join(", "))
        }
        Value::None => "{}".into(),
    }
}

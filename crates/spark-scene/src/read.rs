//! TOML text -> [`SceneFile`].
//!
//! The text is parsed with spans, migrated if it is an older format, then decoded by the registry's field metadata
//! (`FieldType`, units, defaults): the same tables that drive scripts and the inspector. Every error carries the line
//! of the offending key or value.

use spark_core::doc::{
    ComponentError, Environment, FileKind, FileNode, RESERVED_COMPONENTS, SceneFile, StructureError, component_names, normalize_component,
};
use spark_core::registry::registry;
use spark_core::{Fog, MeshSource, NodeId, Sky, Sun};
use spark_reflect::{
    EnumInfo, FieldFlags, FieldInfo, FieldType, Reflect, ReflectEnum, ReflectError, Unit, Value, VariantInfo, did_you_mean, unknown_field,
};

use crate::error::{ErrorKind, SceneError};
use crate::migrate::{FORMAT_VERSION, STEPS, migrate_table};
use crate::prefab::{Target, override_target, split_override_key};
use crate::text::parse_hex_color;
use crate::tree::{Entry, N, Source, Span, Tv, from_table, from_text, to_toml_table};

type R<T> = Result<T, SceneError>;

const NODE_KEYS: &[&str] = &["id", "parent", "name", "visible", "prefab", "tags", "overrides"];

/// A parsed file.
#[derive(Clone, Debug, PartialEq)]
pub struct Loaded {
    pub file: SceneFile,
    /// The `format_version` the text declared (the document itself is always current).
    pub version: u32,
}

impl Loaded {
    /// The file was older than [`FORMAT_VERSION`] and has been migrated in memory.
    pub fn migrated(&self) -> bool {
        self.version < FORMAT_VERSION
    }
}

/// Reads a `.scene` / `.prefab` text. Older formats are migrated; a file newer than this Spark is an error.
pub fn parse(text: &str) -> Result<Loaded, SceneError> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let src = Source::new(text);
    let root = from_text(text).map_err(|(span, msg)| src.error(&span, ErrorKind::Syntax, msg))?;
    let version = header_version(&root, &src)?;
    let (root, reader) = if version < FORMAT_VERSION {
        let mut table = to_toml_table(root.as_table().unwrap_or_default()).map_err(|m| SceneError::new(ErrorKind::Migration, m))?;
        migrate_table(&mut table, version, STEPS, FORMAT_VERSION).map_err(|m| SceneError::new(ErrorKind::Migration, m))?;
        // The migrated tree has no spans: errors lose their line numbers.
        (from_table(&table), Reader { src: Source::new("") })
    } else {
        (root, Reader { src })
    };
    let mut file = reader.file(&root).map_err(|mut e| {
        if version < FORMAT_VERSION {
            e.message.push_str(&format!(" [after migrating the file from format_version {version}]"));
        }
        e
    })?;
    file.comment = leading_comment(text);
    Ok(Loaded { file, version })
}

/// The `format_version` of a file without decoding the rest (cheap; used by `spark migrate --check`).
pub fn read_version(text: &str) -> Result<u32, SceneError> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let src = Source::new(text);
    let root = from_text(text).map_err(|(span, msg)| src.error(&span, ErrorKind::Syntax, msg))?;
    header_version(&root, &src)
}

fn header_version(root: &N, src: &Source) -> R<u32> {
    let header = ["scene", "prefab"].iter().find_map(|k| root.get(k));
    let Some(header) = header else {
        return Err(src.error(
            &None,
            ErrorKind::Invalid,
            "this is not a scene file: it needs a [scene] (or [prefab]) table with a format_version",
        ));
    };
    let Some(version) = header.val.get("format_version") else {
        let found = if header.val.as_table().is_some() { "" } else { " (it must be a table)" };
        return Err(src.error(
            &header.key_span,
            ErrorKind::Invalid,
            format!("[{}] has no `format_version`{found}; this Spark writes {FORMAT_VERSION}", header.key),
        ));
    };
    let Tv::Int(v) = version.val.v else {
        return Err(src.error(&version.val.span, ErrorKind::Invalid, "`format_version` must be an integer"));
    };
    let v = u32::try_from(v).map_err(|_| src.error(&version.val.span, ErrorKind::Invalid, "`format_version` must be 0 or greater"))?;
    if v > FORMAT_VERSION {
        return Err(src.error(
            &version.val.span,
            ErrorKind::TooNew,
            format!(
                "this file has format_version {v}, but this Spark reads up to {FORMAT_VERSION}: update Spark (`spark update`) to open it"
            ),
        ));
    }
    Ok(v)
}

/// The `# ...` lines at the very start of the text.
fn leading_comment(text: &str) -> Vec<String> {
    text.lines().take_while(|l| l.starts_with('#')).map(|l| l.trim_end().to_string()).collect()
}

/// Message for a name that does not exist, with the closest existing ones.
fn name_error(what: &str, name: &str, near: Vec<&'static str>, all: &[&str]) -> String {
    match near.as_slice() {
        [] => format!("unknown {what} `{name}`; expected one of: {}", all.join(", ")),
        [one] => format!("unknown {what} `{name}`; did you mean `{one}`?"),
        many => {
            format!("unknown {what} `{name}`; did you mean {}?", many.iter().map(|s| format!("`{s}`")).collect::<Vec<_>>().join(" or "))
        }
    }
}

struct Reader {
    src: Source,
}

impl Reader {
    fn err(&self, span: &Span, kind: ErrorKind, message: impl Into<String>) -> SceneError {
        self.src.error(span, kind, message)
    }

    // ----- values ------------------------------------------------------------------------------------------

    fn value(&self, ty: &FieldType, info: &FieldInfo, n: &N) -> R<Value> {
        let bad = |expected: &str| {
            self.err(&n.span, ErrorKind::Invalid, format!("`{}`: expected {expected}, found {}", info.name, n.type_name()))
        };
        Ok(match ty {
            FieldType::Bool => match n.v {
                Tv::Bool(b) => Value::Bool(b),
                _ => return Err(bad("true or false")),
            },
            FieldType::Int => match n.v {
                Tv::Int(i) => Value::Int(i),
                _ => return Err(bad("an integer")),
            },
            FieldType::Float => {
                let x = n.as_number().ok_or_else(|| bad("a number"))?;
                // Angles are degrees in files; the registry stores spot angles in radians.
                Value::Float(if info.unit == Some(Unit::Radians) { f64::from((x as f32).to_radians()) } else { x })
            }
            FieldType::Str => match &n.v {
                Tv::Str(s) => Value::Str(s.clone()),
                _ => return Err(bad("a string")),
            },
            FieldType::Vec2 => Value::Vec2(self.floats::<2>(n, info, "[x, y]")?),
            FieldType::Vec3 => Value::Vec3(self.floats::<3>(n, info, "[x, y, z]")?),
            FieldType::Vec4 => Value::Vec4(self.floats::<4>(n, info, "[x, y, z, w]")?),
            FieldType::Color => Value::Color(self.color(n, info.name)?),
            FieldType::Enum(e) => self.enum_value(e(), info, n)?,
            FieldType::Struct(t) => {
                let t = t();
                let entries = n.as_table().ok_or_else(|| bad("a table"))?;
                Value::Map(self.fields(t.name, t.fields, entries, &[])?)
            }
            FieldType::Option(inner) => self.value(inner, info, n)?,
            FieldType::List(inner) => match &n.v {
                Tv::Array(items) => Value::List(items.iter().map(|i| self.value(inner, info, i)).collect::<R<_>>()?),
                _ => return Err(bad("an array")),
            },
            FieldType::NodeRef => match &n.v {
                Tv::Str(s) => Value::NodeRef(s.clone()),
                _ => return Err(bad("a node id (a string)")),
            },
            FieldType::Asset(_) => match &n.v {
                Tv::Str(s) => Value::Asset(s.clone()),
                // The GUID form (analysis 10.4): the path is what is used until `.meta` files exist.
                Tv::Table(_) => match n.get("path").and_then(|p| p.val.as_str()) {
                    Some(p) => Value::Asset(p.to_string()),
                    None => return Err(bad("a path string, or a table with `path`")),
                },
                _ => return Err(bad("a path string")),
            },
        })
    }

    fn floats<const K: usize>(&self, n: &N, info: &FieldInfo, shape: &str) -> R<[f32; K]> {
        let bad = |what: String| self.err(&n.span, ErrorKind::Invalid, format!("`{}`: {what}", info.name));
        let Tv::Array(items) = &n.v else {
            return Err(bad(format!("expected an array of {K} numbers {shape}, found {}", n.type_name())));
        };
        if items.len() != K {
            return Err(bad(format!("expected {K} numbers {shape}, found {}", items.len())));
        }
        let mut out = [0.0; K];
        for (o, i) in out.iter_mut().zip(items) {
            *o = i.as_number().ok_or_else(|| {
                self.err(&i.span, ErrorKind::Invalid, format!("`{}`: expected a number, found {}", info.name, i.type_name()))
            })? as f32;
        }
        Ok(out)
    }

    fn color(&self, n: &N, name: &str) -> R<[f32; 4]> {
        let expected = "a color: \"#rrggbb\", \"#rrggbbaa\" or [r, g, b, a]";
        let bad = |found: &str| self.err(&n.span, ErrorKind::Invalid, format!("`{name}`: expected {expected}, found {found}"));
        match &n.v {
            Tv::Str(s) => parse_hex_color(s).ok_or_else(|| bad(&format!("\"{s}\""))),
            Tv::Array(items) if items.len() == 4 => {
                let mut out = [0.0; 4];
                for (o, i) in out.iter_mut().zip(items) {
                    *o = i.as_number().ok_or_else(|| bad(i.type_name()))? as f32;
                }
                Ok(out)
            }
            Tv::Array(items) => Err(bad(&format!("an array of {}", items.len()))),
            _ => Err(bad(n.type_name())),
        }
    }

    fn variant(&self, e: &'static EnumInfo, name: &str, span: &Span) -> R<&'static VariantInfo> {
        e.variant(name).ok_or_else(|| {
            let all: Vec<&str> = e.variant_names().collect();
            self.err(span, ErrorKind::UnknownName, name_error("value", name, did_you_mean(name, e.variant_names()), &all))
        })
    }

    fn enum_value(&self, e: &'static EnumInfo, info: &FieldInfo, n: &N) -> R<Value> {
        match &n.v {
            Tv::Str(s) => {
                self.variant(e, s, &n.span)?;
                Ok(Value::unit(s))
            }
            Tv::Table(entries) => {
                let all: Vec<&str> = e.variant_names().collect();
                let tag = entries.iter().find(|x| x.key == "type").ok_or_else(|| {
                    self.err(
                        &n.span,
                        ErrorKind::Invalid,
                        format!("`{}`: a table needs `type = \"...\"`, one of: {}", info.name, all.join(", ")),
                    )
                })?;
                let name = tag.val.as_str().ok_or_else(|| self.err(&tag.val.span, ErrorKind::Invalid, "`type` must be a string"))?;
                let v = self.variant(e, name, &tag.val.span)?;
                let fields = self.fields(v.name, v.fields, entries, &["type"])?;
                Ok(Value::Enum { variant: name.to_string(), fields })
            }
            _ => Err(self.err(
                &n.span,
                ErrorKind::Invalid,
                format!("`{}`: expected a string or a table with `type`, found {}", info.name, n.type_name()),
            )),
        }
    }

    /// The keys of a table against a struct's (or enum variant's) fields.
    fn fields(&self, ty: &'static str, fields: &'static [FieldInfo], entries: &[Entry], skip: &[&str]) -> R<Vec<(String, Value)>> {
        let mut out = Vec::new();
        for e in entries {
            if skip.contains(&e.key.as_str()) {
                continue;
            }
            let Some(f) = fields.iter().find(|f| f.name == e.key) else {
                return Err(self.err(&e.key_span, ErrorKind::UnknownName, unknown_field(ty, fields, &e.key).to_string()));
            };
            if f.flags.contains(FieldFlags::TRANSIENT) {
                return Err(self.err(
                    &e.key_span,
                    ErrorKind::Invalid,
                    format!("`{}` is runtime state and cannot be set in a file", f.name),
                ));
            }
            out.push((f.name.to_string(), self.value(&f.ty, f, &e.val)?));
        }
        Ok(out)
    }

    /// Free-form data (`script`, `camera`, `overrides`): TOML types as they are.
    fn generic(&self, n: &N) -> R<Value> {
        Ok(match &n.v {
            Tv::Str(s) => Value::Str(s.clone()),
            Tv::Int(i) => Value::Int(*i),
            Tv::Float(f) => Value::Float(*f),
            Tv::Bool(b) => Value::Bool(*b),
            Tv::Datetime(_) => return Err(self.err(&n.span, ErrorKind::Invalid, "dates are not supported in scene files")),
            Tv::Array(items) => Value::List(items.iter().map(|i| self.generic(i)).collect::<R<_>>()?),
            Tv::Table(entries) => Value::Map(entries.iter().map(|e| Ok((e.key.clone(), self.generic(&e.val)?))).collect::<R<_>>()?),
        })
    }

    // ----- mesh --------------------------------------------------------------------------------------------

    /// `primitive = "box"` + its fields, or `asset = "..."`, `node = "..."`, `part = N`. `skip` are the keys of the
    /// surrounding table that are not part of the recipe (`lod`, `distance`).
    fn recipe(&self, entries: &[Entry], at: &N, skip: &[&str]) -> R<Value> {
        let prim = entries.iter().find(|e| e.key == "primitive");
        let enum_info = MeshSource::enum_info();
        let shapes: Vec<&'static str> = enum_info.variant_names().filter(|n| *n != "asset" && *n != "runtime").collect();
        let variant = match (prim, entries.iter().any(|e| e.key == "asset")) {
            (Some(p), true) if p.val.as_str().is_none() => {
                return Err(self.err(&p.key_span, ErrorKind::Invalid, "a model mesh picks its primitive with `part = N`, not `primitive`"));
            }
            (Some(p), _) => {
                let name = p.val.as_str().ok_or_else(|| self.err(&p.val.span, ErrorKind::Invalid, "`primitive` must be a string"))?;
                if name == "asset" || name == "runtime" {
                    return Err(self.err(
                        &p.val.span,
                        ErrorKind::Invalid,
                        format!(
                            "`primitive` names a generated shape ({}); for a model write `asset = \"path\"` instead",
                            shapes.join(", ")
                        ),
                    ));
                }
                if !shapes.contains(&name) {
                    return Err(self.err(
                        &p.val.span,
                        ErrorKind::UnknownName,
                        name_error("primitive", name, did_you_mean(name, shapes.iter().copied()), &shapes),
                    ));
                }
                name
            }
            (None, true) => "asset",
            (None, false) => {
                return Err(self.err(
                    &at.span,
                    ErrorKind::Invalid,
                    format!("a mesh needs `primitive = \"...\"` ({}) or `asset = \"path\"`", shapes.join(", ")),
                ));
            }
        };
        let vinfo = enum_info.variant(variant).expect("checked above");
        // Candidates for did-you-mean: the variant's fields (the model's `primitive` index is `part` in files).
        let mut known: Vec<&'static str> =
            vinfo.fields.iter().map(|f| if variant == "asset" && f.name == "primitive" { "part" } else { f.name }).collect();
        for extra in ["lod", "distance"] {
            if skip.contains(&extra) {
                known.push(extra);
            }
        }
        if variant != "asset" {
            known.push("primitive");
        }
        let mut fields = Vec::new();
        for e in entries {
            if skip.contains(&e.key.as_str()) || (e.key == "primitive" && variant != "asset") {
                continue;
            }
            let name = if variant == "asset" && e.key == "part" { "primitive" } else { e.key.as_str() };
            if variant == "asset" && e.key == "primitive" {
                return Err(self.err(&e.key_span, ErrorKind::Invalid, "a model mesh picks its primitive with `part = N`, not `primitive`"));
            }
            let Some(f) = vinfo.fields.iter().find(|f| f.name == name) else {
                return Err(self.err(
                    &e.key_span,
                    ErrorKind::UnknownName,
                    name_error("key", &e.key, did_you_mean(&e.key, known.iter().copied()), &known),
                ));
            };
            fields.push((f.name.to_string(), self.value(&f.ty, f, &e.val)?));
        }
        Ok(Value::Enum { variant: variant.to_string(), fields })
    }

    fn mesh(&self, n: &N) -> R<Value> {
        let entries = n.as_table().unwrap_or_default();
        let Value::Enum { variant, mut fields } = self.recipe(entries, n, &["lod"])? else { unreachable!("a recipe is an enum") };
        if let Some(lod) = entries.iter().find(|e| e.key == "lod") {
            let Tv::Array(levels) = &lod.val.v else {
                return Err(self.err(
                    &lod.val.span,
                    ErrorKind::Invalid,
                    format!("`lod`: expected an array of tables, found {}", lod.val.type_name()),
                ));
            };
            let mut out = Vec::new();
            for level in levels {
                let Some(le) = level.as_table() else {
                    return Err(self.err(
                        &level.span,
                        ErrorKind::Invalid,
                        format!("`lod`: each level is a table, found {}", level.type_name()),
                    ));
                };
                let d = le
                    .iter()
                    .find(|e| e.key == "distance")
                    .ok_or_else(|| self.err(&level.span, ErrorKind::Invalid, "a `lod` level needs a `distance` (meters)"))?;
                let dist = d.val.as_number().ok_or_else(|| self.err(&d.val.span, ErrorKind::Invalid, "`distance` must be a number"))?;
                let mesh = self.recipe(le, level, &["distance"])?;
                out.push(Value::Map(vec![("distance".into(), Value::Float(dist)), ("mesh".into(), mesh)]));
            }
            fields.push(("lod".into(), Value::List(out)));
        }
        Ok(Value::Enum { variant, fields })
    }

    // ----- components and nodes ----------------------------------------------------------------------------

    fn component(&self, name: &str, n: &N) -> R<Value> {
        let Some(entries) = n.as_table() else {
            return Err(self.err(&n.span, ErrorKind::Invalid, format!("component `{name}` must be a table: write [node.{name}]")));
        };
        if RESERVED_COMPONENTS.contains(&name) {
            return self.generic(n);
        }
        let info = registry().component(name).expect("the name was checked");
        let partial = if name == "mesh" {
            self.mesh(n)?
        } else {
            let t = info.info();
            Value::Map(self.fields(t.name, t.fields, entries, &[])?)
        };
        normalize_component(name, &partial).map_err(|e| {
            let ComponentError::Invalid(re) = e else { return self.err(&n.span, ErrorKind::Invalid, e.to_string()) };
            let kind = if matches!(re, ReflectError::UnknownField { .. }) { ErrorKind::UnknownName } else { ErrorKind::Invalid };
            self.err(&locate(n, re.path()), kind, re.to_string())
        })
    }

    /// `[node.overrides]`: keys `"<node in the prefab>/<component>.<field>"` (or `"<node>/<component>"` for a whole
    /// component), values decoded like the field they name (a color string becomes a color, a rotation is in degrees).
    fn overrides(&self, n: &N) -> R<Vec<(String, Value)>> {
        let Some(entries) = n.as_table() else {
            return Err(self.err(&n.span, ErrorKind::Invalid, "`overrides` must be a table: write [node.overrides]"));
        };
        let mut out: Vec<(String, Value)> = Vec::new();
        for e in entries {
            let key_err = |m: String| self.err(&e.key_span, ErrorKind::Invalid, m);
            let k = split_override_key(&e.key).map_err(key_err)?;
            if out.iter().any(|(seen, _)| *seen == e.key) {
                return Err(self.err(&e.key_span, ErrorKind::Invalid, format!("the override `{}` is written twice", e.key)));
            }
            let value = match k.field {
                None => match override_target(&k) {
                    Target::Node => {
                        return Err(self.err(
                            &e.key_span,
                            ErrorKind::Invalid,
                            format!("`{}`: `node` has the fields `name` and `visible`: write \"{}/node.name\"", e.key, k.node),
                        ));
                    }
                    Target::Registry | Target::Reserved => self.component(k.component, &e.val)?,
                    Target::Unknown => {
                        let names = component_names();
                        let near = did_you_mean(k.component, names.iter().copied());
                        return Err(self.err(&e.key_span, ErrorKind::UnknownName, name_error("component", k.component, near, &names)));
                    }
                },
                Some("tags") if matches!(override_target(&k), Target::Node) => {
                    let ctx = "`tags` must be an array of strings";
                    let Tv::Array(items) = &e.val.v else { return Err(self.err(&e.val.span, ErrorKind::Invalid, ctx)) };
                    let mut tags = Vec::new();
                    for t in items {
                        let s = t.as_str().ok_or_else(|| self.err(&t.span, ErrorKind::Invalid, ctx))?;
                        tags.push(Value::Str(s.to_string()));
                    }
                    Value::List(tags)
                }
                Some(field) => {
                    let (ty, fields): (&'static str, &'static [FieldInfo]) = match override_target(&k) {
                        Target::Node => {
                            let t = registry().node_info();
                            (t.name, t.fields)
                        }
                        Target::Registry if k.component == "mesh" => {
                            return Err(self.err(
                                &e.key_span,
                                ErrorKind::Invalid,
                                format!(
                                    "`{}`: a mesh is overridden whole: write \"{}/mesh\" = {{ primitive = \"...\", ... }}",
                                    e.key, k.node
                                ),
                            ));
                        }
                        Target::Registry => {
                            let t = registry().component(k.component).expect("checked by override_target").info();
                            (t.name, t.fields)
                        }
                        Target::Reserved => {
                            return Err(self.err(
                                &e.key_span,
                                ErrorKind::Invalid,
                                format!(
                                    "`{}`: `{}` is overridden whole: write \"{}/{}\" = {{ ... }}",
                                    e.key, k.component, k.node, k.component
                                ),
                            ));
                        }
                        Target::Unknown => {
                            let names = component_names();
                            let near = did_you_mean(k.component, names.iter().copied());
                            return Err(self.err(&e.key_span, ErrorKind::UnknownName, name_error("component", k.component, near, &names)));
                        }
                    };
                    let Some(f) = fields.iter().find(|f| f.name == field) else {
                        return Err(self.err(&e.key_span, ErrorKind::UnknownName, unknown_field(ty, fields, field).to_string()));
                    };
                    if f.flags.contains(FieldFlags::TRANSIENT) {
                        return Err(self.err(
                            &e.key_span,
                            ErrorKind::Invalid,
                            format!("`{field}` is runtime state and cannot be set in a file"),
                        ));
                    }
                    self.value(&f.ty, f, &e.val)?
                }
            };
            out.push((e.key.clone(), value));
        }
        Ok(out)
    }

    fn string<'a>(&self, e: &'a Entry) -> R<&'a str> {
        e.val
            .as_str()
            .ok_or_else(|| self.err(&e.val.span, ErrorKind::Invalid, format!("`{}` must be a string, found {}", e.key, e.val.type_name())))
    }

    fn node_id(&self, e: &Entry) -> R<NodeId> {
        NodeId::new(self.string(e)?).map_err(|err| self.err(&e.val.span, ErrorKind::Invalid, err.to_string()))
    }

    /// One `[[node]]`; the spans of its `id` and `parent` values are returned for the structure checks.
    fn node(&self, n: &N) -> R<(FileNode, Span, Span)> {
        let Some(entries) = n.as_table() else {
            return Err(self.err(&n.span, ErrorKind::Invalid, "every `node` entry is a table: write [[node]]"));
        };
        let id_entry =
            entries.iter().find(|e| e.key == "id").ok_or_else(|| self.err(&n.span, ErrorKind::Invalid, "every [[node]] needs an `id`"))?;
        let id = self.node_id(id_entry)?;
        let ctx = format!("node '{id}'");
        let mut node = FileNode::new(id);
        if entries.iter().any(|e| e.key == "prefab") {
            // An instance lists only the components it replaces: the others come from the prefab's root.
            node.components.clear();
        }
        let mut parent_span = None;
        let mut names: Vec<&'static str> = NODE_KEYS.to_vec();
        names.extend(component_names());
        for e in entries {
            let at = |s: String| format!("{ctx}, {s}");
            match e.key.as_str() {
                "id" => {}
                "parent" => {
                    // `"door1/leaf"`: attach inside the prefab instance `door1`, to its node `leaf`.
                    let text = self.string(e).map_err(|x| x.with_context(&ctx))?;
                    let (outer, inner) = match text.split_once('/') {
                        Some((a, b)) => (a, Some(b)),
                        None => (text, None),
                    };
                    let id = |s: &str| {
                        NodeId::new(s).map_err(|err| self.err(&e.val.span, ErrorKind::Invalid, err.to_string()).with_context(&ctx))
                    };
                    node.parent = Some(id(outer)?);
                    node.parent_in = inner.map(id).transpose()?;
                    parent_span = e.val.span.clone();
                }
                "name" => node.name = self.string(e).map_err(|x| x.with_context(&ctx))?.to_string(),
                "visible" => match e.val.v {
                    Tv::Bool(b) => node.visible = b,
                    _ => return Err(self.err(&e.val.span, ErrorKind::Invalid, "`visible` must be true or false").with_context(&ctx)),
                },
                "prefab" => node.prefab = Some(self.string(e).map_err(|x| x.with_context(&ctx))?.to_string()),
                "tags" => {
                    let Tv::Array(items) = &e.val.v else {
                        return Err(self.err(&e.val.span, ErrorKind::Invalid, "`tags` must be an array of strings").with_context(&ctx));
                    };
                    for t in items {
                        let s = t.as_str().ok_or_else(|| {
                            self.err(&t.span, ErrorKind::Invalid, "`tags` must be an array of strings").with_context(&ctx)
                        })?;
                        node.tags.push(s.to_string());
                    }
                }
                "overrides" => {
                    node.overrides = self.overrides(&e.val).map_err(|x| x.with_context(at("[node.overrides]".into())))?;
                }
                name if component_names().contains(&name) => {
                    let v = self.component(name, &e.val).map_err(|x| x.with_context(at(format!("[node.{name}]"))))?;
                    node.set_component(name, v);
                }
                name => {
                    let near = did_you_mean(name, names.iter().copied());
                    return Err(self
                        .err(&e.key_span, ErrorKind::UnknownName, name_error("component or key", name, near, &names))
                        .with_context(&ctx));
                }
            }
        }
        Ok((node, id_entry.val.span.clone(), parent_span))
    }

    fn environment(&self, n: &N) -> R<Environment> {
        let Some(entries) = n.as_table() else {
            return Err(self.err(&n.span, ErrorKind::Invalid, "`environment` must be a table: write [environment]"));
        };
        let mut env = Environment::default();
        let keys = ["ambient", "background", "sun", "fog", "sky"];
        for e in entries {
            let table = |what: &'static str, ty: &'static spark_reflect::TypeInfo| -> R<Value> {
                let Some(sub) = e.val.as_table() else {
                    return Err(self.err(&e.val.span, ErrorKind::Invalid, format!("`{what}` must be a table: write [environment.{what}]")));
                };
                Ok(Value::Map(self.fields(ty.name, ty.fields, sub, &[])?))
            };
            let located = |re: ReflectError| {
                let kind = if matches!(re, ReflectError::UnknownField { .. }) { ErrorKind::UnknownName } else { ErrorKind::Invalid };
                self.err(&locate(&e.val, re.path()), kind, re.to_string())
            };
            match e.key.as_str() {
                "ambient" => env.ambient = color_of(self.color(&e.val, "ambient")?),
                "background" => env.background = color_of(self.color(&e.val, "background")?),
                "sun" => env.sun = Sun::from_value(&table("sun", Sun::type_info())?).map_err(located)?,
                "fog" => env.fog = Some(Fog::from_value(&table("fog", Fog::type_info())?).map_err(located)?),
                "sky" => env.sky = Some(Sky::from_value(&table("sky", Sky::type_info())?).map_err(located)?),
                other => {
                    return Err(self
                        .err(&e.key_span, ErrorKind::UnknownName, name_error("key", other, did_you_mean(other, keys), &keys))
                        .with_context("[environment]"));
                }
            }
        }
        Ok(env)
    }

    fn file(&self, root: &N) -> R<SceneFile> {
        let entries = root.as_table().unwrap_or_default();
        let keys = ["scene", "prefab", "environment", "node"];
        let mut kind = None;
        let mut file = SceneFile::new(FileKind::Scene);
        let mut env: Option<(&Entry, Environment)> = None;
        let mut nodes: Vec<(FileNode, Span, Span)> = Vec::new();
        for e in entries {
            match e.key.as_str() {
                "scene" | "prefab" => {
                    if kind.is_some() {
                        return Err(self.err(&e.key_span, ErrorKind::Invalid, "a file has either [scene] or [prefab], not both"));
                    }
                    kind = Some(if e.key == "scene" { FileKind::Scene } else { FileKind::Prefab });
                    let header = e
                        .val
                        .as_table()
                        .ok_or_else(|| self.err(&e.val.span, ErrorKind::Invalid, format!("[{}] must be a table", e.key)))?;
                    let hkeys = ["format_version", "name"];
                    for h in header {
                        match h.key.as_str() {
                            "format_version" => {}
                            "name" => file.name = self.string(h)?.to_string(),
                            other => {
                                let msg = name_error("key", other, did_you_mean(other, hkeys), &hkeys);
                                return Err(self.err(&h.key_span, ErrorKind::UnknownName, msg).with_context(format!("[{}]", e.key)));
                            }
                        }
                    }
                }
                "environment" => env = Some((e, self.environment(&e.val)?)),
                "node" => {
                    let Tv::Array(items) = &e.val.v else {
                        return Err(self.err(&e.key_span, ErrorKind::Invalid, "`node` must be an array of tables: write [[node]]"));
                    };
                    for item in items {
                        nodes.push(self.node(item)?);
                    }
                }
                other => {
                    return Err(self.err(
                        &e.key_span,
                        ErrorKind::UnknownName,
                        name_error("top-level table", other, did_you_mean(other, keys), &keys),
                    ));
                }
            }
        }
        file.kind = kind.ok_or_else(|| self.err(&None, ErrorKind::Invalid, "the file has no [scene] or [prefab] table"))?;
        if let Some((e, env)) = env {
            if file.kind == FileKind::Prefab {
                return Err(self.err(&e.key_span, ErrorKind::Invalid, "a prefab has no [environment]: lighting belongs to the scene"));
            }
            file.environment = env;
        }
        file.nodes = nodes.iter().map(|(n, ..)| n.clone()).collect();
        if let Some(issue) = file.check_structure().into_iter().next() {
            let (_, id_span, parent_span) = &nodes[issue.index];
            let span = if matches!(issue.error, StructureError::DanglingParent { .. }) { parent_span } else { id_span };
            let id = file.nodes[issue.index].id.to_string();
            return Err(self.err(span, ErrorKind::Structure, issue.error.to_string()).with_context(format!("node '{id}'")));
        }
        Ok(file)
    }
}

fn color_of(c: [f32; 4]) -> spark_core::Color {
    spark_core::Color::rgba(c[0], c[1], c[2], c[3])
}

/// The span of the deepest key or element `path` (`kind.angle`, `lod[1].mesh`) reaches inside `n`.
fn locate(n: &N, path: &str) -> Span {
    let mut cur = n;
    let mut best = n.span.clone();
    for seg in path.split('.').filter(|s| !s.is_empty()) {
        let (key, indexes) = match seg.find('[') {
            Some(i) => (&seg[..i], &seg[i..]),
            None => (seg, ""),
        };
        if !key.is_empty() {
            let Some(e) = cur.get(key) else { return best };
            best = e.key_span.clone().or_else(|| e.val.span.clone());
            cur = &e.val;
        }
        for idx in indexes.split(['[', ']']).filter(|s| !s.is_empty()) {
            let (Ok(i), Tv::Array(items)) = (idx.parse::<usize>(), &cur.v) else { return best };
            let Some(item) = items.get(i) else { return best };
            best = item.span.clone().or(best);
            cur = item;
        }
    }
    best
}

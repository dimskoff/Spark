//! Static metadata: types, fields, enums, flags, units. Also the validation driven by that metadata.

use std::fmt;
use std::ops::BitOr;

use crate::error::ReflectError;
use crate::value::Value;

/// Kind of project asset an `Asset` field points at (drives the inspector's asset picker).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AssetKind {
    Mesh,
    Texture,
    Sound,
    Script,
    Prefab,
    Model,
    Shader,
    Cubemap,
}

impl AssetKind {
    /// Parses the lowercase name used in `#[reflect(asset = "texture")]`.
    pub fn parse(s: &str) -> Option<AssetKind> {
        Some(match s {
            "mesh" => AssetKind::Mesh,
            "texture" => AssetKind::Texture,
            "sound" => AssetKind::Sound,
            "script" => AssetKind::Script,
            "prefab" => AssetKind::Prefab,
            "model" => AssetKind::Model,
            "shader" => AssetKind::Shader,
            "cubemap" => AssetKind::Cubemap,
            _ => return None,
        })
    }
}

/// Physical unit of a numeric field (shown by the inspector next to the value).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unit {
    Meters,
    Seconds,
    Kilograms,
    /// Stored in radians, shown as degrees by the inspector.
    Radians,
    Degrees,
    MetersPerSecond,
    RadiansPerSecond,
    Hertz,
    Percent,
}

impl Unit {
    /// Parses the symbol used in `#[reflect(unit = "kg")]`.
    pub fn parse(s: &str) -> Option<Unit> {
        Some(match s {
            "m" => Unit::Meters,
            "s" => Unit::Seconds,
            "kg" => Unit::Kilograms,
            "rad" => Unit::Radians,
            "deg" => Unit::Degrees,
            "m/s" => Unit::MetersPerSecond,
            "rad/s" => Unit::RadiansPerSecond,
            "Hz" => Unit::Hertz,
            "%" => Unit::Percent,
            _ => return None,
        })
    }

    pub fn symbol(self) -> &'static str {
        match self {
            Unit::Meters => "m",
            Unit::Seconds => "s",
            Unit::Kilograms => "kg",
            Unit::Radians => "rad",
            Unit::Degrees => "deg",
            Unit::MetersPerSecond => "m/s",
            Unit::RadiansPerSecond => "rad/s",
            Unit::Hertz => "Hz",
            Unit::Percent => "%",
        }
    }
}

/// Per-field flags.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub struct FieldFlags(u8);

impl FieldFlags {
    pub const NONE: FieldFlags = FieldFlags(0);
    /// Cannot be changed through `set` (whole-value decoding still fills it).
    pub const READ_ONLY: FieldFlags = FieldFlags(1);
    /// Not shown in the inspector.
    pub const HIDDEN: FieldFlags = FieldFlags(2);
    /// Runtime only: not saved to scene files.
    pub const TRANSIENT: FieldFlags = FieldFlags(4);
    /// No flat Luau accessor is generated.
    pub const NO_LUA: FieldFlags = FieldFlags(8);
    /// Out-of-range numbers are clamped into `range` instead of being rejected.
    pub const CLAMP: FieldFlags = FieldFlags(16);

    pub const fn contains(self, other: FieldFlags) -> bool {
        self.0 & other.0 == other.0
    }
    pub const fn union(self, other: FieldFlags) -> FieldFlags {
        FieldFlags(self.0 | other.0)
    }
}

impl BitOr for FieldFlags {
    type Output = FieldFlags;
    fn bitor(self, rhs: FieldFlags) -> FieldFlags {
        self.union(rhs)
    }
}

impl fmt::Debug for FieldFlags {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let names = [
            (FieldFlags::READ_ONLY, "READ_ONLY"),
            (FieldFlags::HIDDEN, "HIDDEN"),
            (FieldFlags::TRANSIENT, "TRANSIENT"),
            (FieldFlags::NO_LUA, "NO_LUA"),
            (FieldFlags::CLAMP, "CLAMP"),
        ];
        let set: Vec<&str> = names.iter().filter(|(fl, _)| self.contains(*fl)).map(|(_, n)| *n).collect();
        if set.is_empty() { write!(f, "NONE") } else { write!(f, "{}", set.join(" | ")) }
    }
}

/// Type of a field. `Struct` and `Enum` hold accessor functions (not references) because a `static`
/// initializer cannot call `Reflect::type_info()` directly.
#[derive(Clone, Copy)]
pub enum FieldType {
    Bool,
    Int,
    Float,
    Str,
    Vec2,
    Vec3,
    Vec4,
    Color,
    Enum(fn() -> &'static EnumInfo),
    Struct(fn() -> &'static TypeInfo),
    Option(&'static FieldType),
    List(&'static FieldType),
    NodeRef,
    Asset(AssetKind),
}

impl FieldType {
    /// Short lowercase name used in error messages (matches `Value::type_name` for the plain kinds).
    pub fn name(&self) -> &'static str {
        match self {
            FieldType::Bool => "bool",
            FieldType::Int => "int",
            FieldType::Float => "float",
            FieldType::Str => "string",
            FieldType::Vec2 => "vec2",
            FieldType::Vec3 => "vec3",
            FieldType::Vec4 => "vec4",
            FieldType::Color => "color",
            FieldType::Enum(_) => "enum",
            FieldType::Struct(_) => "struct",
            FieldType::Option(_) => "option",
            FieldType::List(_) => "list",
            FieldType::NodeRef => "node_ref",
            FieldType::Asset(_) => "asset",
        }
    }

    /// Retags a plain value to this type's representation: `Vec4` -> `Color`, `Str` -> `Asset` / `NodeRef`
    /// (also inside `Option` and `List`). Used when a field is declared `color` / `asset` / `node_ref`
    /// on top of a `[f32; 4]` / `String` Rust type.
    pub fn tag(&self, v: Value) -> Value {
        match (self, v) {
            (FieldType::Color, Value::Vec4(a)) => Value::Color(a),
            (FieldType::Asset(_), Value::Str(s)) => Value::Asset(s),
            (FieldType::NodeRef, Value::Str(s)) => Value::NodeRef(s),
            (FieldType::Option(inner), v) if v != Value::None => inner.tag(v),
            (FieldType::List(inner), Value::List(items)) => Value::List(items.into_iter().map(|i| inner.tag(i)).collect()),
            (_, v) => v,
        }
    }
}

impl fmt::Debug for FieldType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FieldType::Enum(e) => write!(f, "Enum({})", e().name),
            FieldType::Struct(t) => write!(f, "Struct({})", t().name),
            FieldType::Option(i) => write!(f, "Option({i:?})"),
            FieldType::List(i) => write!(f, "List({i:?})"),
            FieldType::Asset(k) => write!(f, "Asset({k:?})"),
            other => write!(f, "{}", other.name()),
        }
    }
}

/// Metadata of one field.
pub struct FieldInfo {
    /// File / inspector / path name.
    pub name: &'static str,
    pub ty: FieldType,
    /// From the `///` comment.
    pub doc: &'static str,
    /// From `Default` or `#[reflect(default = ...)]`.
    pub default: fn() -> Value,
    /// Inclusive; `None` = unbounded. Floats must be finite regardless.
    pub range: Option<(f64, f64)>,
    /// Inspector drag step.
    pub step: Option<f64>,
    pub unit: Option<Unit>,
    pub flags: FieldFlags,
    /// Flat Luau name on `Object`, e.g. `Material.color` -> `"color"`.
    pub lua: Option<&'static str>,
}

impl fmt::Debug for FieldInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FieldInfo")
            .field("name", &self.name)
            .field("ty", &self.ty)
            .field("doc", &self.doc)
            .field("default", &(self.default)())
            .field("range", &self.range)
            .field("step", &self.step)
            .field("unit", &self.unit)
            .field("flags", &self.flags)
            .field("lua", &self.lua)
            .finish()
    }
}

impl FieldInfo {
    /// Validates `v` against this field and returns the normalized value (integers widened to floats,
    /// strings tagged as assets, out-of-range numbers clamped when the field has the `CLAMP` flag).
    /// Checks type, range and finiteness; nested structs and enums are only checked for their shape here
    /// and fully validated when they are decoded.
    pub fn check(&self, path: &str, v: &Value) -> Result<Value, ReflectError> {
        check_type(self, &self.ty, path, v)
    }
}

fn range_f(info: &FieldInfo, path: &str, x: f64) -> Result<f64, ReflectError> {
    if !x.is_finite() {
        return Err(ReflectError::NotFinite { path: path.to_string() });
    }
    match info.range {
        Some((min, max)) if x < min || x > max => {
            if info.flags.contains(FieldFlags::CLAMP) {
                Ok(x.clamp(min, max))
            } else {
                Err(ReflectError::OutOfRange { path: path.to_string(), value: x, min, max })
            }
        }
        _ => Ok(x),
    }
}

fn range_f32(info: &FieldInfo, path: &str, x: f32) -> Result<f32, ReflectError> {
    Ok(range_f(info, path, f64::from(x))? as f32)
}

fn wrong(path: &str, ty: &FieldType, v: &Value) -> ReflectError {
    ReflectError::WrongType { path: path.to_string(), expected: ty.name(), got: v.type_name() }
}

fn check_type(info: &FieldInfo, ty: &FieldType, path: &str, v: &Value) -> Result<Value, ReflectError> {
    let comps = |a: &[f32]| -> Result<Vec<f32>, ReflectError> {
        a.iter()
            .enumerate()
            .map(|(i, c)| {
                let comp = ["x", "y", "z", "w"][i];
                let p = if path.is_empty() { comp.to_string() } else { format!("{path}.{comp}") };
                range_f32(info, &p, *c)
            })
            .collect()
    };
    Ok(match (ty, v) {
        (FieldType::Bool, Value::Bool(_)) => v.clone(),
        (FieldType::Int, Value::Int(i)) => {
            let x = range_f(info, path, *i as f64)?;
            // `x` is the original value unless it was clamped.
            Value::Int(if x == *i as f64 { *i } else { x.round() as i64 })
        }
        (FieldType::Int, Value::Float(f)) if f.is_finite() && f.fract() == 0.0 => {
            let x = range_f(info, path, *f)?;
            Value::Int(x as i64)
        }
        (FieldType::Float, Value::Float(f)) => Value::Float(range_f(info, path, *f)?),
        (FieldType::Float, Value::Int(i)) => Value::Float(range_f(info, path, *i as f64)?),
        (FieldType::Str, Value::Str(_)) => v.clone(),
        (FieldType::Vec2, Value::Vec2(a)) => {
            let c = comps(a)?;
            Value::Vec2([c[0], c[1]])
        }
        (FieldType::Vec3, Value::Vec3(a)) => {
            let c = comps(a)?;
            Value::Vec3([c[0], c[1], c[2]])
        }
        (FieldType::Vec4 | FieldType::Color, Value::Vec4(a) | Value::Color(a)) => {
            let c = comps(a)?;
            let out = [c[0], c[1], c[2], c[3]];
            if matches!(ty, FieldType::Color) { Value::Color(out) } else { Value::Vec4(out) }
        }
        (FieldType::Enum(_), Value::Enum { .. }) => v.clone(),
        (FieldType::Enum(_), Value::Str(s)) => Value::unit(s),
        (FieldType::Struct(_), Value::Map(_)) => v.clone(),
        (FieldType::Option(_), Value::None) => Value::None,
        (FieldType::Option(inner), other) => check_type(info, inner, path, other)?,
        (FieldType::List(inner), Value::List(items)) => Value::List(
            items.iter().enumerate().map(|(i, item)| check_type(info, inner, &format!("{path}[{i}]"), item)).collect::<Result<_, _>>()?,
        ),
        (FieldType::NodeRef, Value::NodeRef(s) | Value::Str(s)) => Value::NodeRef(s.clone()),
        (FieldType::Asset(_), Value::Asset(s) | Value::Str(s)) => Value::Asset(s.clone()),
        _ => return Err(wrong(path, ty, v)),
    })
}

/// Metadata of an enum variant (unit variants have no fields).
#[derive(Debug)]
pub struct VariantInfo {
    pub name: &'static str,
    pub doc: &'static str,
    pub fields: &'static [FieldInfo],
}

/// Metadata of an enum.
#[derive(Debug)]
pub struct EnumInfo {
    pub name: &'static str,
    pub doc: &'static str,
    pub variants: &'static [VariantInfo],
}

impl EnumInfo {
    pub fn variant(&self, name: &str) -> Option<&'static VariantInfo> {
        self.variants.iter().find(|v| v.name == name)
    }
    pub fn variant_names(&self) -> impl Iterator<Item = &'static str> {
        self.variants.iter().map(|v| v.name)
    }
}

/// Metadata of a struct.
#[derive(Debug)]
pub struct TypeInfo {
    /// "Body"
    pub name: &'static str,
    pub doc: &'static str,
    pub fields: &'static [FieldInfo],
}

impl TypeInfo {
    pub fn field(&self, name: &str) -> Option<&'static FieldInfo> {
        self.fields.iter().find(|f| f.name == name)
    }
    pub fn field_names(&self) -> impl Iterator<Item = &'static str> {
        self.fields.iter().map(|f| f.name)
    }
}

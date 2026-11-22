//! `ReflectField` (values that can live in a reflected field) and the helpers the derive expands to.
//! The helpers are public so that a type can also be reflected by hand (see `tests/common/mod.rs`).

use crate::error::{ReflectError, did_you_mean};
use crate::info::{EnumInfo, FieldFlags, FieldInfo, FieldType, TypeInfo};
use crate::value::Value;

/// Ordered name/value pairs: the payload of `Value::Map` and of an enum variant.
pub type Pairs = [(String, Value)];

/// A reflected struct: a named collection of fields.
pub trait Reflect: Sized + 'static {
    fn type_info() -> &'static TypeInfo;
    fn to_value(&self) -> Value;
    /// Whole-value decode with validation (type, range, finite). Missing fields take defaults,
    /// unknown fields are an error.
    fn from_value(v: &Value) -> Result<Self, ReflectError>;
    /// Path access: `"mass"`, `"shape.radius"`. The empty path is the whole value.
    fn get(&self, path: &str) -> Result<Value, ReflectError>;
    /// Path write with validation. The empty path replaces the whole value.
    fn set(&mut self, path: &str, v: Value) -> Result<(), ReflectError>;
}

/// A reflected enum: unit variants and/or variants with named fields.
pub trait ReflectEnum: Sized + 'static {
    fn enum_info() -> &'static EnumInfo;
    /// Name of the current variant.
    fn variant_name(&self) -> &'static str;
    fn to_value(&self) -> Value;
    fn from_value(v: &Value) -> Result<Self, ReflectError>;
    /// `"radius"` addresses a field of the current variant.
    fn get(&self, path: &str) -> Result<Value, ReflectError>;
    fn set(&mut self, path: &str, v: Value) -> Result<(), ReflectError>;
}

/// A Rust type that can be the type of a reflected field. Implemented here for the primitives, `[f32; N]`,
/// `String`, `Option<T>` and `Vec<T>`; the derives implement it for reflected structs and enums.
pub trait ReflectField: Sized {
    const FIELD_TYPE: FieldType;
    fn to_val(&self) -> Value;
    /// `v` has already been normalized by `FieldInfo::check` (but a mismatch is still reported, not a panic).
    fn from_val(v: &Value) -> Result<Self, ReflectError>;
    /// Reads below this value (`rest` is non-empty). Only structs, enums, vectors and `Option` support it.
    fn get_path(&self, rest: &str) -> Result<Value, ReflectError> {
        Err(not_a_struct(&Self::FIELD_TYPE, rest))
    }
    fn set_path(&mut self, rest: &str, _v: Value) -> Result<(), ReflectError> {
        Err(not_a_struct(&Self::FIELD_TYPE, rest))
    }
}

fn not_a_struct(ty: &FieldType, rest: &str) -> ReflectError {
    ReflectError::WrongType { path: split_path(rest).0.to_string(), expected: "struct", got: ty.name() }
}

fn mismatch(expected: &FieldType, v: &Value) -> ReflectError {
    ReflectError::WrongType { path: String::new(), expected: expected.name(), got: v.type_name() }
}

// ---------------------------------------------------------------------------------------------------------
// Helpers used by the derive output (and by hand-written impls)

/// `"shape.radius"` -> `("shape", "radius")`; `"mass"` -> `("mass", "")`.
pub fn split_path(path: &str) -> (&str, &str) {
    path.split_once('.').unwrap_or((path, ""))
}

/// Error for a name that is not a field of `fields`, with suggestions.
pub fn unknown_field(ty: &'static str, fields: &'static [FieldInfo], path: &str) -> ReflectError {
    ReflectError::UnknownField { ty, path: path.to_string(), suggestions: did_you_mean(path, fields.iter().map(|f| f.name)) }
}

/// `ty.tag(value.to_val())`: the default value of a field as `FieldInfo::default` returns it.
pub fn tagged<T: ReflectField>(ty: FieldType, value: &T) -> Value {
    ty.tag(value.to_val())
}

/// Encodes a field value (applies `color` / `asset` / `node_ref` tagging).
pub fn encode_field<T: ReflectField>(info: &FieldInfo, value: &T) -> Value {
    info.ty.tag(value.to_val())
}

/// Decodes one field of a map: missing -> the field default, then validate and convert.
pub fn decode_field<T: ReflectField>(info: &FieldInfo, map: &[(String, Value)]) -> Result<T, ReflectError> {
    let given = map.iter().find(|(k, _)| k == info.name).map(|(_, v)| v);
    let v = match given {
        Some(v) => v.clone(),
        None => (info.default)(),
    };
    let checked = info.check("", &v).map_err(|e| e.at(info.name))?;
    T::from_val(&checked).map_err(|e| e.at(info.name))
}

/// Path read of one field. `rest` is what remains after the field name.
pub fn get_field<T: ReflectField>(info: &FieldInfo, slot: &T, rest: &str) -> Result<Value, ReflectError> {
    if rest.is_empty() { Ok(encode_field(info, slot)) } else { slot.get_path(rest).map_err(|e| e.at(info.name)) }
}

/// Path write of one field: read-only check, validation (type, range, finite), then assignment.
pub fn set_field<T: ReflectField>(info: &FieldInfo, slot: &mut T, rest: &str, v: Value) -> Result<(), ReflectError> {
    if info.flags.contains(FieldFlags::READ_ONLY) {
        return Err(ReflectError::ReadOnly { path: info.name.to_string() });
    }
    if !rest.is_empty() {
        return slot.set_path(rest, v).map_err(|e| e.at(info.name));
    }
    let checked = info.check("", &v).map_err(|e| e.at(info.name))?;
    *slot = T::from_val(&checked).map_err(|e| e.at(info.name))?;
    Ok(())
}

/// Checks that `v` is a map holding only known fields of `info`.
pub fn expect_map<'a>(info: &'static TypeInfo, v: &'a Value) -> Result<&'a [(String, Value)], ReflectError> {
    let Value::Map(pairs) = v else {
        return Err(ReflectError::WrongType { path: String::new(), expected: "map", got: v.type_name() });
    };
    check_keys(info.name, info.fields, pairs)?;
    Ok(pairs)
}

fn check_keys(ty: &'static str, fields: &'static [FieldInfo], pairs: &[(String, Value)]) -> Result<(), ReflectError> {
    for (k, _) in pairs {
        if !fields.iter().any(|f| f.name == k) {
            return Err(unknown_field(ty, fields, k));
        }
    }
    Ok(())
}

/// Splits an enum value into (variant index, its fields). A plain string names a unit variant.
pub fn expect_enum<'a>(info: &'static EnumInfo, v: &'a Value) -> Result<(usize, &'a Pairs), ReflectError> {
    let (name, fields): (&str, &[(String, Value)]) = match v {
        Value::Enum { variant, fields } => (variant, fields),
        Value::Str(s) => (s, &[]),
        other => {
            return Err(ReflectError::WrongType { path: String::new(), expected: "enum", got: other.type_name() });
        }
    };
    let Some(idx) = info.variants.iter().position(|x| x.name == name) else {
        return Err(ReflectError::UnknownField {
            ty: info.name,
            path: name.to_string(),
            suggestions: did_you_mean(name, info.variant_names()),
        });
    };
    let vinfo = &info.variants[idx];
    check_keys(vinfo.name, vinfo.fields, fields)?;
    Ok((idx, fields))
}

// ---------------------------------------------------------------------------------------------------------
// Primitive impls

impl ReflectField for bool {
    const FIELD_TYPE: FieldType = FieldType::Bool;
    fn to_val(&self) -> Value {
        Value::Bool(*self)
    }
    fn from_val(v: &Value) -> Result<Self, ReflectError> {
        match v {
            Value::Bool(b) => Ok(*b),
            _ => Err(mismatch(&Self::FIELD_TYPE, v)),
        }
    }
}

macro_rules! impl_int {
    ($($t:ty),*) => {$(
        impl ReflectField for $t {
            const FIELD_TYPE: FieldType = FieldType::Int;
            fn to_val(&self) -> Value {
                Value::Int(i64::from(*self))
            }
            fn from_val(v: &Value) -> Result<Self, ReflectError> {
                match v {
                    Value::Int(i) => <$t>::try_from(*i).map_err(|_| ReflectError::OutOfRange {
                        path: String::new(),
                        value: *i as f64,
                        min: <$t>::MIN as f64,
                        max: <$t>::MAX as f64,
                    }),
                    _ => Err(mismatch(&Self::FIELD_TYPE, v)),
                }
            }
        }
    )*};
}
impl_int!(i8, i16, i32, i64, u8, u16, u32);

impl ReflectField for f32 {
    const FIELD_TYPE: FieldType = FieldType::Float;
    fn to_val(&self) -> Value {
        Value::Float(f64::from(*self))
    }
    fn from_val(v: &Value) -> Result<Self, ReflectError> {
        match v {
            Value::Float(f) => {
                let x = *f as f32;
                if x.is_finite() {
                    Ok(x)
                } else if f.is_finite() {
                    Err(ReflectError::OutOfRange { path: String::new(), value: *f, min: f64::from(f32::MIN), max: f64::from(f32::MAX) })
                } else {
                    Err(ReflectError::NotFinite { path: String::new() })
                }
            }
            _ => Err(mismatch(&Self::FIELD_TYPE, v)),
        }
    }
}

impl ReflectField for f64 {
    const FIELD_TYPE: FieldType = FieldType::Float;
    fn to_val(&self) -> Value {
        Value::Float(*self)
    }
    fn from_val(v: &Value) -> Result<Self, ReflectError> {
        match v {
            Value::Float(f) if f.is_finite() => Ok(*f),
            Value::Float(_) => Err(ReflectError::NotFinite { path: String::new() }),
            _ => Err(mismatch(&Self::FIELD_TYPE, v)),
        }
    }
}

impl ReflectField for String {
    const FIELD_TYPE: FieldType = FieldType::Str;
    fn to_val(&self) -> Value {
        Value::Str(self.clone())
    }
    fn from_val(v: &Value) -> Result<Self, ReflectError> {
        match v {
            Value::Str(s) | Value::Asset(s) | Value::NodeRef(s) => Ok(s.clone()),
            _ => Err(mismatch(&Self::FIELD_TYPE, v)),
        }
    }
}

const COMPONENTS: [&str; 4] = ["x", "y", "z", "w"];

fn component(rest: &str, len: usize) -> Result<usize, ReflectError> {
    let (head, tail) = split_path(rest);
    if !tail.is_empty() {
        return Err(ReflectError::WrongType { path: head.to_string(), expected: "struct", got: "float" });
    }
    match COMPONENTS[..len].iter().position(|c| *c == head) {
        Some(i) => Ok(i),
        None => Err(ReflectError::UnknownField {
            ty: ["", "", "vec2", "vec3", "vec4"][len],
            path: head.to_string(),
            suggestions: did_you_mean(head, COMPONENTS[..len].iter().copied()),
        }),
    }
}

macro_rules! impl_vec {
    ($n:literal, $variant:ident, $ty:expr) => {
        impl ReflectField for [f32; $n] {
            const FIELD_TYPE: FieldType = $ty;
            fn to_val(&self) -> Value {
                Value::$variant(*self)
            }
            fn from_val(v: &Value) -> Result<Self, ReflectError> {
                match v {
                    Value::$variant(a) => Ok(*a),
                    _ if $n == 4 => match v {
                        Value::Vec4(a) | Value::Color(a) => {
                            let mut out = [0.0; $n];
                            out.copy_from_slice(a);
                            Ok(out)
                        }
                        _ => Err(mismatch(&Self::FIELD_TYPE, v)),
                    },
                    _ => Err(mismatch(&Self::FIELD_TYPE, v)),
                }
            }
            fn get_path(&self, rest: &str) -> Result<Value, ReflectError> {
                Ok(Value::Float(f64::from(self[component(rest, $n)?])))
            }
            fn set_path(&mut self, rest: &str, v: Value) -> Result<(), ReflectError> {
                let i = component(rest, $n)?;
                match v {
                    Value::Float(f) if f.is_finite() => self[i] = f as f32,
                    Value::Int(n) => self[i] = n as f32,
                    Value::Float(_) => {
                        return Err(ReflectError::NotFinite { path: COMPONENTS[i].to_string() });
                    }
                    other => {
                        return Err(ReflectError::WrongType { path: COMPONENTS[i].to_string(), expected: "float", got: other.type_name() });
                    }
                }
                Ok(())
            }
        }
    };
}
impl_vec!(2, Vec2, FieldType::Vec2);
impl_vec!(3, Vec3, FieldType::Vec3);
impl_vec!(4, Vec4, FieldType::Vec4);

impl<T: ReflectField> ReflectField for Option<T> {
    const FIELD_TYPE: FieldType = FieldType::Option(&T::FIELD_TYPE);
    fn to_val(&self) -> Value {
        match self {
            Some(x) => x.to_val(),
            None => Value::None,
        }
    }
    fn from_val(v: &Value) -> Result<Self, ReflectError> {
        match v {
            Value::None => Ok(None),
            other => T::from_val(other).map(Some),
        }
    }
    fn get_path(&self, rest: &str) -> Result<Value, ReflectError> {
        match self {
            Some(x) => x.get_path(rest),
            None => Err(ReflectError::WrongType { path: String::new(), expected: "struct", got: "none" }),
        }
    }
    fn set_path(&mut self, rest: &str, v: Value) -> Result<(), ReflectError> {
        match self {
            Some(x) => x.set_path(rest, v),
            None => Err(ReflectError::WrongType { path: String::new(), expected: "struct", got: "none" }),
        }
    }
}

impl<T: ReflectField> ReflectField for Vec<T> {
    const FIELD_TYPE: FieldType = FieldType::List(&T::FIELD_TYPE);
    fn to_val(&self) -> Value {
        Value::List(self.iter().map(T::to_val).collect())
    }
    fn from_val(v: &Value) -> Result<Self, ReflectError> {
        match v {
            Value::List(items) => items.iter().enumerate().map(|(i, x)| T::from_val(x).map_err(|e| e.at(&format!("[{i}]")))).collect(),
            _ => Err(mismatch(&Self::FIELD_TYPE, v)),
        }
    }
}

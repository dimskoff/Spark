//! Building blocks for Luau type declarations (`docs/spark.d.luau`) and docs generated from field metadata.
//!
//! This module knows nothing about the engine's components: it maps a [`FieldType`] to Luau type text and a
//! [`FieldInfo`] to a doc comment (documentation, range, unit, default). The component registry lives in `spark-core`,
//! so the generator that walks it (`spark-api`, ADR 0001 amended by ADR 0004) calls these functions.

use crate::info::{AssetKind, FieldInfo, FieldType, Unit};
use crate::value::Value;

/// Which side of the API a type is written for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Flavor {
    /// What reading a field gives: `Color`, `vector`.
    Read,
    /// What a spawn / spec table accepts: `ColorLike`, every key optional.
    Spec,
}

/// Luau type text of a field type.
///
/// `aliases` maps an enum name of the registry to a named Luau alias that is declared by hand (`("BodyKind",
/// "BodyType")`); any other enum is written as a union of its variant names. In [`Flavor::Spec`] the result is not yet
/// optional: see [`optional`].
pub fn luau_type(ty: &FieldType, flavor: Flavor, aliases: &[(&str, &str)]) -> String {
    match ty {
        FieldType::Bool => "boolean".into(),
        FieldType::Int | FieldType::Float => "number".into(),
        FieldType::Str | FieldType::NodeRef => "string".into(),
        FieldType::Vec2 | FieldType::Vec3 | FieldType::Vec4 => "vector".into(),
        FieldType::Color => (if flavor == Flavor::Spec { "ColorLike" } else { "Color" }).into(),
        FieldType::Enum(info) => {
            let info = info();
            match aliases.iter().find(|(n, _)| *n == info.name) {
                Some((_, alias)) => (*alias).into(),
                None => info.variants.iter().map(|v| format!("\"{}\"", v.name)).collect::<Vec<_>>().join(" | "),
            }
        }
        FieldType::Struct(info) => info().name.into(),
        FieldType::Option(inner) => optional(&luau_type(inner, flavor, aliases)),
        FieldType::List(inner) => format!("{{ {} }}", luau_type(inner, flavor, aliases)),
        FieldType::Asset(kind) => match kind {
            AssetKind::Mesh => "Mesh",
            AssetKind::Texture => "Texture",
            AssetKind::Sound => "Sound",
            AssetKind::Model => "Model",
            AssetKind::Shader => "Shader",
            AssetKind::Script | AssetKind::Prefab | AssetKind::Cubemap => "string",
        }
        .into(),
    }
}

/// `T?`; a union gets parentheses (`("a" | "b")?`), an already optional type stays as it is.
pub fn optional(ty: &str) -> String {
    if ty.ends_with('?') {
        ty.to_string()
    } else if ty.contains(" | ") && !ty.starts_with('(') {
        format!("({ty})?")
    } else {
        format!("{ty}?")
    }
}

/// How a value reads in a doc comment (`0.5`, `true`, `"point"`, `vec3(0, 0, 1)`); `None` for values that say nothing
/// (`nil`, empty lists and strings, handles).
pub fn describe_value(v: &Value) -> Option<String> {
    let num = |x: f64| {
        let f = x as f32;
        if f.fract() == 0.0 && f.abs() < 1.0e9 { format!("{}", f as i64) } else { format!("{f}") }
    };
    let vec = |c: &[f32]| c.iter().map(|x| num(f64::from(*x))).collect::<Vec<_>>().join(", ");
    match v {
        Value::Bool(b) => Some(b.to_string()),
        Value::Int(i) => Some(i.to_string()),
        Value::Float(f) => Some(num(*f)),
        Value::Str(s) | Value::Asset(s) | Value::NodeRef(s) if !s.is_empty() => Some(format!("\"{s}\"")),
        Value::Vec2(c) => Some(format!("vec2({})", vec(c))),
        Value::Vec3(c) => Some(format!("vec3({})", vec(c))),
        Value::Vec4(c) => Some(format!("vec4({})", vec(c))),
        Value::Color(c) => Some(format!("rgba({})", vec(c))),
        Value::Enum { variant, .. } => Some(format!("\"{variant}\"")),
        _ => None,
    }
}

/// `"Range 0 to 1."`, `"At least 0.01."`, `"Greater than 0."`; `None` when the field is unbounded (or when the
/// range is given in an angle unit that scripts see differently: radians for the inspector, degrees in Luau).
pub fn describe_range(f: &FieldInfo) -> Option<String> {
    let (min, max) = f.range?;
    if matches!(f.unit, Some(Unit::Radians | Unit::Degrees)) {
        return None;
    }
    let num = |x: f64| describe_value(&Value::Float(x)).unwrap_or_default();
    let unbounded_hi = max >= f64::from(f32::MAX) * 0.99;
    let tiny_lo = min > 0.0 && min < 1.0e-30;
    let unit = match f.unit {
        Some(u) if !matches!(u, Unit::Radians | Unit::Degrees) => format!(" {}", u.symbol()),
        _ => String::new(),
    };
    Some(match (tiny_lo, unbounded_hi) {
        (true, true) => "Greater than 0.".to_string(),
        (true, false) => format!("Greater than 0, at most {}{unit}.", num(max)),
        (false, true) if min == 0.0 => "At least 0.".to_string(),
        (false, true) => format!("At least {}{unit}.", num(min)),
        (false, false) => format!("Range {} to {}{unit}.", num(min), num(max)),
    })
}

/// Rust doc text as Luau docs: intra-doc links lose their brackets (`[`Material::pbr`]` -> `Material::pbr`) and the
/// Rust `None` reads `nil`.
pub fn luau_doc(text: &str) -> String {
    text.replace("[`", "`").replace("`]", "`").replace("`None`", "`nil`")
}

/// Doc comment lines of a field (without the `--` prefix): the `///` text (or `doc` when given: the Luau API words
/// some fields differently, e.g. radians), then the unit, the range and the default as short sentences.
/// `with_default = false` leaves the default out (read handles).
pub fn field_doc(f: &FieldInfo, doc: Option<&str>, with_default: bool) -> Vec<String> {
    let text = luau_doc(doc.unwrap_or(f.doc));
    let mut lines: Vec<String> = text.lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect();
    let angle = matches!(f.unit, Some(Unit::Radians | Unit::Degrees));
    let mut facts: Vec<String> = Vec::new();
    match f.unit {
        Some(u) if !angle && f.range.is_none() => facts.push(format!("Unit: {}.", u.symbol())),
        _ => {}
    }
    if let Some(r) = describe_range(f) {
        facts.push(r);
    }
    // Angles are radians in the inspector's registry but their own thing in Luau: no default text.
    let default = if with_default && !angle { describe_value(&(f.default)()) } else { None };
    if let Some(d) = default {
        facts.push(format!("Default {d}."));
    }
    if !facts.is_empty() {
        lines.push(facts.join(" "));
    }
    lines
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn optional_wraps_unions() {
        assert_eq!(optional("number"), "number?");
        assert_eq!(optional("number?"), "number?");
        assert_eq!(optional("\"a\" | \"b\""), "(\"a\" | \"b\")?");
        assert_eq!(optional("(number | vector)?"), "(number | vector)?");
    }

    #[test]
    fn values_read_like_luau() {
        assert_eq!(describe_value(&Value::Float(0.5)).as_deref(), Some("0.5"));
        assert_eq!(describe_value(&Value::Float(2.0)).as_deref(), Some("2"));
        assert_eq!(describe_value(&Value::Vec3([0.0, 1.0, 0.5])).as_deref(), Some("vec3(0, 1, 0.5)"));
        assert_eq!(describe_value(&Value::None), None);
        assert_eq!(describe_value(&Value::Str(String::new())), None);
    }
}

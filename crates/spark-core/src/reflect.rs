//! Glue for the reflection of engine data (ADR 0004): `ReflectField` for [`Color`] and for the runtime handles
//! ([`MeshId`], [`TextureId`], [`CubemapId`], [`ShaderId`]), and the constants the `#[reflect(range = ...)]`
//! attributes use.

use spark_reflect::{AssetKind, FieldType, ReflectError, ReflectField, Value};

use crate::assets::{CubemapId, MeshId, TextureId};
use crate::color::Color;
use crate::shader::ShaderId;

/// Smallest positive `f32`: an inclusive range bound that means "greater than 0" for `f32` values (radii, masses,
/// the clip planes), which is what the scripting API checks (`positive`). Used as
/// `range = GREATER_THAN_ZERO..=f32::MAX`: `0.0` is rejected, any positive `f32` (denormals too) is accepted.
pub(crate) const GREATER_THAN_ZERO: f32 = f32::from_bits(1);

/// Colors are four floats (`r g b a`), reflected as `Value::Color`.
impl ReflectField for Color {
    const FIELD_TYPE: FieldType = FieldType::Color;
    fn to_val(&self) -> Value {
        Value::Color([self.r, self.g, self.b, self.a])
    }
    fn from_val(v: &Value) -> Result<Self, ReflectError> {
        match v {
            Value::Color([r, g, b, a]) | Value::Vec4([r, g, b, a]) => Ok(Color::rgba(*r, *g, *b, *a)),
            other => Err(ReflectError::WrongType { path: String::new(), expected: "color", got: other.type_name() }),
        }
    }
}

macro_rules! impl_handle_field {
    ($t:ty, $kind:ident, $label:literal) => {
        /// A runtime handle. Until assets have a project path (mesh recipes, step C-11) it is shown as
        /// `runtime:<kind>/<index>` and cannot be created from a value, so fields that hold one are
        /// `readonly` and `transient`.
        impl ReflectField for $t {
            const FIELD_TYPE: FieldType = FieldType::Asset(AssetKind::$kind);
            fn to_val(&self) -> Value {
                Value::Asset(format!(concat!("runtime:", $label, "/{}"), self.0))
            }
            fn from_val(v: &Value) -> Result<Self, ReflectError> {
                Err(ReflectError::WrongType {
                    path: String::new(),
                    expected: concat!("a ", $label, " handle (cannot be created from a value yet)"),
                    got: v.type_name(),
                })
            }
        }
    };
}
impl_handle_field!(MeshId, Mesh, "mesh");
impl_handle_field!(TextureId, Texture, "texture");
impl_handle_field!(CubemapId, Cubemap, "cubemap");
impl_handle_field!(ShaderId, Shader, "shader");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_roundtrip() {
        let c = Color::rgba(0.1, 0.2, 0.3, 0.4);
        assert_eq!(c.to_val(), Value::Color([0.1, 0.2, 0.3, 0.4]));
        assert_eq!(Color::from_val(&c.to_val()), Ok(c));
        assert!(Color::from_val(&Value::Float(1.0)).is_err());
    }

    #[test]
    fn handles_are_shown_but_not_created() {
        assert_eq!(TextureId(3).to_val(), Value::Asset("runtime:texture/3".into()));
        assert!(TextureId::from_val(&Value::Asset("runtime:texture/3".into())).is_err());
        assert_eq!(GREATER_THAN_ZERO.to_bits(), 1);
    }
}

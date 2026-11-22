//! Spark Engine reflection: one description of a component field (name, type, range, unit, doc, default,
//! flags, Luau name) that the Luau bindings, the inspector, scene files, docs and devtools all read.
//!
//! No dependencies, no GPU, no serde. See `docs/dev/adr/0004-type-registry-reflection.md`.
//!
//! - [`Value`]: the dynamic value exchanged with the outside world.
//! - [`TypeInfo`] / [`FieldInfo`] / [`EnumInfo`]: static metadata.
//! - [`Reflect`] / [`ReflectEnum`]: whole-value and path access with validation (type, range, finite).
//! - [`ReflectError`]: errors with a did-you-mean list.
//! - `#[derive(Reflect)]` / `#[derive(ReflectEnum)]` (feature `derive`, on by default) generate all of it.
#![forbid(unsafe_code)]

mod error;
mod field;
mod info;
mod value;

pub use error::{ReflectError, did_you_mean};
pub use field::{
    Reflect, ReflectEnum, ReflectField, decode_field, encode_field, expect_enum, expect_map, get_field, set_field, split_path, tagged,
    unknown_field,
};
pub use info::{AssetKind, EnumInfo, FieldFlags, FieldInfo, FieldType, TypeInfo, Unit, VariantInfo};
pub use value::Value;

#[cfg(feature = "derive")]
pub use spark_reflect_derive::{Reflect, ReflectEnum};

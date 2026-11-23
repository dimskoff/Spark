//! Shared test fixtures: hand-written `Reflect` impls (what the derive expands to, written with the public
//! helpers) and a list of operations that `derive_equiv.rs` replays on the derived twin types.
#![allow(dead_code)]

use spark_reflect::{
    AssetKind, EnumInfo, FieldFlags, FieldInfo, FieldType, Reflect, ReflectEnum, ReflectError, ReflectField, TypeInfo, Unit, Value,
    VariantInfo, decode_field, encode_field, expect_enum, expect_map, get_field, set_field, split_path, tagged, unknown_field,
};

const fn plain(name: &'static str, ty: FieldType, doc: &'static str, default: fn() -> Value) -> FieldInfo {
    FieldInfo { name, ty, doc, default, range: None, step: None, unit: None, flags: FieldFlags::NONE, lua: None }
}

pub mod hand {
    use super::*;

    /// How the body moves.
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub enum BodyKind {
        Static,
        Dynamic,
        Kinematic,
    }

    static BODY_KIND: EnumInfo = EnumInfo {
        name: "BodyKind",
        doc: "How the body moves.",
        variants: &[
            VariantInfo { name: "Static", doc: "", fields: &[] },
            VariantInfo { name: "Dynamic", doc: "Moved by forces.", fields: &[] },
            VariantInfo { name: "Kinematic", doc: "", fields: &[] },
        ],
    };

    impl ReflectEnum for BodyKind {
        fn enum_info() -> &'static EnumInfo {
            &BODY_KIND
        }
        fn variant_name(&self) -> &'static str {
            match self {
                BodyKind::Static => "Static",
                BodyKind::Dynamic => "Dynamic",
                BodyKind::Kinematic => "Kinematic",
            }
        }
        fn to_value(&self) -> Value {
            Value::unit(self.variant_name())
        }
        fn from_value(v: &Value) -> Result<Self, ReflectError> {
            let (idx, _) = expect_enum(&BODY_KIND, v)?;
            Ok([BodyKind::Static, BodyKind::Dynamic, BodyKind::Kinematic][idx])
        }
        fn get(&self, path: &str) -> Result<Value, ReflectError> {
            if path.is_empty() {
                return Ok(self.to_value());
            }
            Err(unknown_field("BodyKind", &[], split_path(path).0))
        }
        fn set(&mut self, path: &str, v: Value) -> Result<(), ReflectError> {
            if path.is_empty() {
                *self = Self::from_value(&v)?;
                return Ok(());
            }
            Err(unknown_field("BodyKind", &[], split_path(path).0))
        }
    }

    impl ReflectField for BodyKind {
        const FIELD_TYPE: FieldType = FieldType::Enum(<BodyKind as ReflectEnum>::enum_info);
        fn to_val(&self) -> Value {
            ReflectEnum::to_value(self)
        }
        fn from_val(v: &Value) -> Result<Self, ReflectError> {
            <BodyKind as ReflectEnum>::from_value(v)
        }
        fn get_path(&self, rest: &str) -> Result<Value, ReflectError> {
            ReflectEnum::get(self, rest)
        }
        fn set_path(&mut self, rest: &str, v: Value) -> Result<(), ReflectError> {
            ReflectEnum::set(self, rest, v)
        }
    }

    /// Collision shape.
    #[derive(Clone, Debug, PartialEq)]
    pub enum Shape {
        Sphere { radius: f32 },
        Box { half: [f32; 3] },
        Point,
    }

    static SHAPE: EnumInfo = EnumInfo {
        name: "Shape",
        doc: "Collision shape.",
        variants: &[
            VariantInfo {
                name: "Sphere",
                doc: "A ball.",
                fields: &[FieldInfo {
                    range: Some((0.001, 1000.0)),
                    unit: Some(Unit::Meters),
                    ..plain("radius", FieldType::Float, "Radius.", || Value::Float(0.5))
                }],
            },
            VariantInfo {
                name: "Box",
                doc: "",
                fields: &[FieldInfo {
                    range: Some((0.001, 1000.0)),
                    unit: Some(Unit::Meters),
                    ..plain("half", FieldType::Vec3, "Half extents.", || Value::Vec3([0.5, 0.5, 0.5]))
                }],
            },
            VariantInfo { name: "Point", doc: "", fields: &[] },
        ],
    };

    impl ReflectEnum for Shape {
        fn enum_info() -> &'static EnumInfo {
            &SHAPE
        }
        fn variant_name(&self) -> &'static str {
            match self {
                Shape::Sphere { .. } => "Sphere",
                Shape::Box { .. } => "Box",
                Shape::Point => "Point",
            }
        }
        fn to_value(&self) -> Value {
            let fields = match self {
                Shape::Sphere { radius } => {
                    vec![("radius".to_string(), encode_field(&SHAPE.variants[0].fields[0], radius))]
                }
                Shape::Box { half } => {
                    vec![("half".to_string(), encode_field(&SHAPE.variants[1].fields[0], half))]
                }
                Shape::Point => Vec::new(),
            };
            Value::Enum { variant: self.variant_name().to_string(), fields }
        }
        fn from_value(v: &Value) -> Result<Self, ReflectError> {
            let (idx, fields) = expect_enum(&SHAPE, v)?;
            Ok(match idx {
                0 => Shape::Sphere { radius: decode_field(&SHAPE.variants[0].fields[0], fields)? },
                1 => Shape::Box { half: decode_field(&SHAPE.variants[1].fields[0], fields)? },
                _ => Shape::Point,
            })
        }
        fn get(&self, path: &str) -> Result<Value, ReflectError> {
            if path.is_empty() {
                return Ok(self.to_value());
            }
            let (head, rest) = split_path(path);
            match self {
                Shape::Sphere { radius } if head == "radius" => get_field(&SHAPE.variants[0].fields[0], radius, rest),
                Shape::Box { half } if head == "half" => get_field(&SHAPE.variants[1].fields[0], half, rest),
                Shape::Sphere { .. } => Err(unknown_field("Shape", SHAPE.variants[0].fields, head)),
                Shape::Box { .. } => Err(unknown_field("Shape", SHAPE.variants[1].fields, head)),
                Shape::Point => Err(unknown_field("Shape", SHAPE.variants[2].fields, head)),
            }
        }
        fn set(&mut self, path: &str, v: Value) -> Result<(), ReflectError> {
            if path.is_empty() {
                *self = Self::from_value(&v)?;
                return Ok(());
            }
            let (head, rest) = split_path(path);
            match self {
                Shape::Sphere { radius } if head == "radius" => set_field(&SHAPE.variants[0].fields[0], radius, rest, v),
                Shape::Box { half } if head == "half" => set_field(&SHAPE.variants[1].fields[0], half, rest, v),
                Shape::Sphere { .. } => Err(unknown_field("Shape", SHAPE.variants[0].fields, head)),
                Shape::Box { .. } => Err(unknown_field("Shape", SHAPE.variants[1].fields, head)),
                Shape::Point => Err(unknown_field("Shape", SHAPE.variants[2].fields, head)),
            }
        }
    }

    impl ReflectField for Shape {
        const FIELD_TYPE: FieldType = FieldType::Enum(<Shape as ReflectEnum>::enum_info);
        fn to_val(&self) -> Value {
            ReflectEnum::to_value(self)
        }
        fn from_val(v: &Value) -> Result<Self, ReflectError> {
            <Shape as ReflectEnum>::from_value(v)
        }
        fn get_path(&self, rest: &str) -> Result<Value, ReflectError> {
            ReflectEnum::get(self, rest)
        }
        fn set_path(&mut self, rest: &str, v: Value) -> Result<(), ReflectError> {
            ReflectEnum::set(self, rest, v)
        }
    }

    /// Rigid body simulated by physics.
    #[derive(Clone, Debug, PartialEq)]
    pub struct Body {
        /// How the body moves.
        pub kind: BodyKind,
        /// Total mass; `None` = computed from volume.
        pub mass: Option<f32>,
        /// Restitution.
        pub bounciness: f32,
        /// Surface roughness (clamped).
        pub roughness: f32,
        /// World-space velocity.
        pub velocity: [f32; 3],
        /// Bumped by the simulation.
        pub generation: u32,
        /// Collision shape.
        pub shape: Shape,
        /// Tint (linear RGBA).
        pub tint: [f32; 4],
        /// Collision layers.
        pub layers: Vec<i32>,
        /// Optional texture.
        pub texture: Option<String>,
        /// Editor-only label.
        pub label: String,
    }

    impl Default for Body {
        fn default() -> Self {
            Body {
                kind: BodyKind::Dynamic,
                mass: None,
                bounciness: 0.3,
                roughness: 0.5,
                velocity: [0.0; 3],
                generation: 0,
                shape: Shape::Sphere { radius: 0.5 },
                tint: [1.0; 4],
                layers: Vec::new(),
                texture: None,
                label: String::new(),
            }
        }
    }

    const BODY_TINT_TY: FieldType = FieldType::Color;
    const BODY_TEXTURE_TY: FieldType = FieldType::Option(&FieldType::Asset(AssetKind::Texture));

    static BODY: TypeInfo = TypeInfo {
        name: "Body",
        doc: "Rigid body simulated by physics.",
        fields: &[
            plain("kind", <BodyKind as ReflectField>::FIELD_TYPE, "How the body moves.", || {
                tagged(<BodyKind as ReflectField>::FIELD_TYPE, &Body::default().kind)
            }),
            FieldInfo {
                range: Some((0.001, 1.0e6)),
                unit: Some(Unit::Kilograms),
                ..plain("mass", <Option<f32> as ReflectField>::FIELD_TYPE, "Total mass; `None` = computed from volume.", || {
                    tagged(<Option<f32> as ReflectField>::FIELD_TYPE, &Body::default().mass)
                })
            },
            FieldInfo {
                range: Some((0.0, 1.0)),
                step: Some(0.01),
                ..plain("bounciness", FieldType::Float, "Restitution.", || Value::Float(f64::from(0.3_f32)))
            },
            FieldInfo {
                range: Some((0.0, 1.0)),
                flags: FieldFlags::CLAMP,
                ..plain("roughness", FieldType::Float, "Surface roughness (clamped).", || Value::Float(0.5))
            },
            FieldInfo {
                unit: Some(Unit::MetersPerSecond),
                flags: FieldFlags::TRANSIENT,
                lua: Some("velocity"),
                ..plain("velocity", FieldType::Vec3, "World-space velocity.", || Value::Vec3([0.0; 3]))
            },
            FieldInfo {
                flags: FieldFlags::READ_ONLY.union(FieldFlags::HIDDEN),
                ..plain("generation", FieldType::Int, "Bumped by the simulation.", || Value::Int(0))
            },
            plain("shape", <Shape as ReflectField>::FIELD_TYPE, "Collision shape.", || {
                tagged(<Shape as ReflectField>::FIELD_TYPE, &Body::default().shape)
            }),
            plain("tint", BODY_TINT_TY, "Tint (linear RGBA).", || Value::Color([1.0; 4])),
            FieldInfo {
                range: Some((0.0, 31.0)),
                ..plain("layers", <Vec<i32> as ReflectField>::FIELD_TYPE, "Collision layers.", || Value::List(Vec::new()))
            },
            plain("texture", BODY_TEXTURE_TY, "Optional texture.", || Value::None),
            FieldInfo { flags: FieldFlags::NO_LUA, ..plain("label", FieldType::Str, "Editor-only label.", || Value::Str(String::new())) },
        ],
    };

    impl Reflect for Body {
        fn type_info() -> &'static TypeInfo {
            &BODY
        }
        fn to_value(&self) -> Value {
            let f = BODY.fields;
            Value::Map(vec![
                ("kind".into(), encode_field(&f[0], &self.kind)),
                ("mass".into(), encode_field(&f[1], &self.mass)),
                ("bounciness".into(), encode_field(&f[2], &self.bounciness)),
                ("roughness".into(), encode_field(&f[3], &self.roughness)),
                ("velocity".into(), encode_field(&f[4], &self.velocity)),
                ("generation".into(), encode_field(&f[5], &self.generation)),
                ("shape".into(), encode_field(&f[6], &self.shape)),
                ("tint".into(), encode_field(&f[7], &self.tint)),
                ("layers".into(), encode_field(&f[8], &self.layers)),
                ("texture".into(), encode_field(&f[9], &self.texture)),
                ("label".into(), encode_field(&f[10], &self.label)),
            ])
        }
        fn from_value(v: &Value) -> Result<Self, ReflectError> {
            let f = BODY.fields;
            let m = expect_map(&BODY, v)?;
            Ok(Body {
                kind: decode_field(&f[0], m)?,
                mass: decode_field(&f[1], m)?,
                bounciness: decode_field(&f[2], m)?,
                roughness: decode_field(&f[3], m)?,
                velocity: decode_field(&f[4], m)?,
                generation: decode_field(&f[5], m)?,
                shape: decode_field(&f[6], m)?,
                tint: decode_field(&f[7], m)?,
                layers: decode_field(&f[8], m)?,
                texture: decode_field(&f[9], m)?,
                label: decode_field(&f[10], m)?,
            })
        }
        fn get(&self, path: &str) -> Result<Value, ReflectError> {
            if path.is_empty() {
                return Ok(self.to_value());
            }
            let f = BODY.fields;
            let (head, rest) = split_path(path);
            match head {
                "kind" => get_field(&f[0], &self.kind, rest),
                "mass" => get_field(&f[1], &self.mass, rest),
                "bounciness" => get_field(&f[2], &self.bounciness, rest),
                "roughness" => get_field(&f[3], &self.roughness, rest),
                "velocity" => get_field(&f[4], &self.velocity, rest),
                "generation" => get_field(&f[5], &self.generation, rest),
                "shape" => get_field(&f[6], &self.shape, rest),
                "tint" => get_field(&f[7], &self.tint, rest),
                "layers" => get_field(&f[8], &self.layers, rest),
                "texture" => get_field(&f[9], &self.texture, rest),
                "label" => get_field(&f[10], &self.label, rest),
                _ => Err(unknown_field("Body", f, head)),
            }
        }
        fn set(&mut self, path: &str, v: Value) -> Result<(), ReflectError> {
            if path.is_empty() {
                *self = Self::from_value(&v)?;
                return Ok(());
            }
            let f = BODY.fields;
            let (head, rest) = split_path(path);
            match head {
                "kind" => set_field(&f[0], &mut self.kind, rest, v),
                "mass" => set_field(&f[1], &mut self.mass, rest, v),
                "bounciness" => set_field(&f[2], &mut self.bounciness, rest, v),
                "roughness" => set_field(&f[3], &mut self.roughness, rest, v),
                "velocity" => set_field(&f[4], &mut self.velocity, rest, v),
                "generation" => set_field(&f[5], &mut self.generation, rest, v),
                "shape" => set_field(&f[6], &mut self.shape, rest, v),
                "tint" => set_field(&f[7], &mut self.tint, rest, v),
                "layers" => set_field(&f[8], &mut self.layers, rest, v),
                "texture" => set_field(&f[9], &mut self.texture, rest, v),
                "label" => set_field(&f[10], &mut self.label, rest, v),
                _ => Err(unknown_field("Body", f, head)),
            }
        }
    }

    impl ReflectField for Body {
        const FIELD_TYPE: FieldType = FieldType::Struct(<Body as Reflect>::type_info);
        fn to_val(&self) -> Value {
            Reflect::to_value(self)
        }
        fn from_val(v: &Value) -> Result<Self, ReflectError> {
            <Body as Reflect>::from_value(v)
        }
        fn get_path(&self, rest: &str) -> Result<Value, ReflectError> {
            Reflect::get(self, rest)
        }
        fn set_path(&mut self, rest: &str, v: Value) -> Result<(), ReflectError> {
            Reflect::set(self, rest, v)
        }
    }
}

/// Operations replayed on both the hand-written and the derived `Body`.
pub fn ops() -> Vec<(&'static str, Value)> {
    let box_shape = Value::Enum { variant: "Box".into(), fields: vec![("half".into(), Value::Vec3([1.0, 2.0, 3.0]))] };
    vec![
        ("shape.radius", Value::Float(2.0)),
        ("shape.radius", Value::Float(0.0)),
        ("shape.radius", Value::Float(f64::NAN)),
        ("shape.radus", Value::Float(1.0)),
        ("shape.half", Value::Vec3([1.0; 3])),
        ("mass", Value::Float(5.0)),
        ("mass", Value::Float(0.0)),
        ("mass", Value::Float(f64::INFINITY)),
        ("mass", Value::Str("heavy".into())),
        ("mass", Value::None),
        ("mass.x", Value::Float(1.0)),
        ("bounciness", Value::Float(0.75)),
        ("bounciness", Value::Int(1)),
        ("bounciness", Value::Float(1.5)),
        ("bouncines", Value::Float(0.5)),
        ("roughness", Value::Float(7.0)),
        ("roughness", Value::Float(-3.0)),
        ("roughness", Value::Float(f64::NAN)),
        ("velocity", Value::Vec3([1.0, 2.0, 3.0])),
        ("velocity.y", Value::Float(9.0)),
        ("velocity.y", Value::Float(f64::NAN)),
        ("velocity.q", Value::Float(1.0)),
        ("velocity", Value::Vec3([f32::NAN, 0.0, 0.0])),
        ("generation", Value::Int(3)),
        ("kind", Value::Str("Static".into())),
        ("kind", Value::Str("Statik".into())),
        ("kind", Value::unit("Kinematic")),
        ("shape", box_shape),
        ("shape.radius", Value::Float(1.0)),
        ("shape.half.y", Value::Float(4.0)),
        ("shape", Value::unit("Point")),
        ("tint", Value::Color([0.5, 0.25, 0.125, 1.0])),
        ("tint", Value::Vec4([1.0, 0.0, 0.0, 1.0])),
        ("tint.w", Value::Float(0.5)),
        ("layers", Value::List(vec![Value::Int(1), Value::Int(31)])),
        ("layers", Value::List(vec![Value::Int(1), Value::Int(40)])),
        ("texture", Value::Str("tex/brick.png".into())),
        ("label", Value::Str("crate".into())),
        ("nope", Value::Bool(true)),
    ]
}

/// Applies `ops` to a default value; returns every result and the final `to_value`.
pub fn replay<T: Reflect + Default>(ops: &[(&str, Value)]) -> (Vec<Result<(), ReflectError>>, Value) {
    let mut t = T::default();
    let results = ops.iter().map(|(p, v)| t.set(p, v.clone())).collect();
    (results, t.to_value())
}

/// Debug rendering of the metadata (field names, types, docs, defaults, ranges, units, flags, lua names).
pub fn describe<T: Reflect>() -> String {
    format!("{:#?}", T::type_info())
}

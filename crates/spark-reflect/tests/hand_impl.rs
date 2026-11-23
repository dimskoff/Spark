//! Hand-written `Reflect` impls: path get/set, validation (NaN, range, clamp, read-only, type), did-you-mean,
//! and `from_value(to_value(x)) == x`.

mod common;

use common::hand::{Body, BodyKind, Shape};
use spark_reflect::{Reflect, ReflectEnum, ReflectError, Value};

fn f(x: f64) -> Value {
    Value::Float(x)
}

#[test]
fn get_by_path() {
    let b = Body::default();
    assert_eq!(b.get("mass"), Ok(Value::None));
    assert_eq!(b.get("bounciness"), Ok(f(f64::from(0.3_f32))));
    assert_eq!(b.get("shape.radius"), Ok(f(0.5)));
    assert_eq!(b.get("velocity.y"), Ok(f(0.0)));
    assert_eq!(b.get("kind"), Ok(Value::unit("Dynamic")));
    assert_eq!(b.get("tint"), Ok(Value::Color([1.0; 4])));
    assert_eq!(b.get("texture"), Ok(Value::None));
    assert_eq!(b.get(""), Ok(b.to_value()));
}

#[test]
fn set_by_path() {
    let mut b = Body::default();
    b.set("shape.radius", f(2.0)).unwrap();
    assert_eq!(b.shape, Shape::Sphere { radius: 2.0 });
    assert_eq!(b.get("shape.radius"), Ok(f(2.0)));

    b.set("velocity.y", f(3.0)).unwrap();
    assert_eq!(b.velocity, [0.0, 3.0, 0.0]);

    b.set("mass", f(5.0)).unwrap();
    assert_eq!(b.mass, Some(5.0));
    b.set("mass", Value::None).unwrap();
    assert_eq!(b.mass, None);

    b.set("bounciness", Value::Int(1)).unwrap(); // int widened to float
    assert_eq!(b.bounciness, 1.0);

    b.set("kind", Value::Str("Static".into())).unwrap();
    assert_eq!(b.kind, BodyKind::Static);

    b.set("tint", Value::Vec4([0.0, 0.5, 1.0, 1.0])).unwrap(); // a vec4 is accepted for a color
    assert_eq!(b.get("tint"), Ok(Value::Color([0.0, 0.5, 1.0, 1.0])));

    b.set("texture", Value::Str("tex/a.png".into())).unwrap();
    assert_eq!(b.get("texture"), Ok(Value::Asset("tex/a.png".into())));

    let box_shape = Value::Enum { variant: "Box".into(), fields: vec![("half".into(), Value::Vec3([1.0, 2.0, 3.0]))] };
    b.set("shape", box_shape).unwrap();
    assert_eq!(b.shape, Shape::Box { half: [1.0, 2.0, 3.0] });
    b.set("shape.half.y", f(4.0)).unwrap();
    assert_eq!(b.shape, Shape::Box { half: [1.0, 4.0, 3.0] });
}

#[test]
fn nan_and_inf_are_rejected_and_leave_the_value_untouched() {
    let mut b = Body::default();
    let before = b.clone();
    for (path, v) in [
        ("bounciness", f(f64::NAN)),
        ("shape.radius", f(f64::INFINITY)),
        ("velocity", Value::Vec3([f32::NAN, 0.0, 0.0])),
        ("velocity.x", f(f64::NAN)),
        ("mass", f(f64::NEG_INFINITY)),
        ("roughness", f(f64::NAN)), // even a clamped field
    ] {
        let err = b.set(path, v).unwrap_err();
        assert!(matches!(err, ReflectError::NotFinite { .. }), "{path}: {err:?}");
    }
    assert_eq!(b.set("velocity", Value::Vec3([0.0, f32::INFINITY, 0.0])), Err(ReflectError::NotFinite { path: "velocity.y".into() }));
    assert_eq!(b, before);
}

#[test]
fn out_of_range_is_rejected() {
    let mut b = Body::default();
    assert_eq!(b.set("bounciness", f(1.5)), Err(ReflectError::OutOfRange { path: "bounciness".into(), value: 1.5, min: 0.0, max: 1.0 }));
    assert_eq!(
        b.set("shape.radius", f(0.0)),
        Err(ReflectError::OutOfRange { path: "shape.radius".into(), value: 0.0, min: 0.001, max: 1000.0 })
    );
    // Option<f32>: the range applies to the inner value.
    assert!(matches!(b.set("mass", f(0.0)), Err(ReflectError::OutOfRange { .. })));
    // Lists: per element, with the index in the path.
    assert_eq!(
        b.set("layers", Value::List(vec![Value::Int(1), Value::Int(40)])),
        Err(ReflectError::OutOfRange { path: "layers[1]".into(), value: 40.0, min: 0.0, max: 31.0 })
    );
    assert_eq!(b, Body::default());
}

#[test]
fn clamp_flag_clamps_instead_of_failing() {
    let mut b = Body::default();
    b.set("roughness", f(7.0)).unwrap();
    assert_eq!(b.roughness, 1.0);
    b.set("roughness", f(-3.0)).unwrap();
    assert_eq!(b.roughness, 0.0);
    b.set("roughness", f(0.25)).unwrap();
    assert_eq!(b.roughness, 0.25);
}

#[test]
fn unknown_field_gives_a_suggestion() {
    let mut b = Body::default();
    let err = b.set("bouncines", f(0.5)).unwrap_err();
    assert_eq!(err, ReflectError::UnknownField { ty: "Body", path: "bouncines".into(), suggestions: vec!["bounciness"] });
    assert_eq!(err.to_string(), "unknown field `bouncines` on Body; did you mean `bounciness`?");

    // Nested: the type is the one that owns the field, the path is the full path.
    assert_eq!(
        b.get("shape.radus"),
        Err(ReflectError::UnknownField { ty: "Shape", path: "shape.radus".into(), suggestions: vec!["radius"] })
    );
    // Vector components.
    assert!(matches!(b.get("velocity.q"), Err(ReflectError::UnknownField { ty: "vec3", .. })));
    // Enum variants.
    let err = b.set("kind", Value::Str("Statik".into())).unwrap_err();
    assert_eq!(err, ReflectError::UnknownField { ty: "BodyKind", path: "kind.Statik".into(), suggestions: vec!["Static"] });
    // Nothing close: no suggestion.
    let Err(ReflectError::UnknownField { suggestions, .. }) = b.get("zzzzzzzz") else { panic!("expected UnknownField") };
    assert!(suggestions.is_empty());
}

#[test]
fn wrong_type_read_only_and_non_struct_paths() {
    let mut b = Body::default();
    assert_eq!(
        b.set("bounciness", Value::Str("x".into())),
        Err(ReflectError::WrongType { path: "bounciness".into(), expected: "float", got: "string" })
    );
    assert_eq!(b.set("velocity", f(1.0)), Err(ReflectError::WrongType { path: "velocity".into(), expected: "vec3", got: "float" }));
    assert_eq!(b.set("generation", Value::Int(3)), Err(ReflectError::ReadOnly { path: "generation".into() }));
    assert_eq!(b.generation, 0);
    assert!(matches!(b.get("bounciness.x"), Err(ReflectError::WrongType { expected: "struct", .. })));
    // Going through `None`.
    assert!(matches!(b.get("mass.x"), Err(ReflectError::WrongType { got: "none", .. })));
    // The wrong variant's field.
    assert!(matches!(b.get("shape.half"), Err(ReflectError::UnknownField { ty: "Shape", .. })));
}

#[test]
fn from_value_to_value_roundtrip() {
    let default = Body::default();
    assert_eq!(Body::from_value(&default.to_value()), Ok(default.clone()));

    let custom = Body {
        kind: BodyKind::Kinematic,
        mass: Some(12.5),
        bounciness: 0.8,
        roughness: 0.125,
        velocity: [1.0, -2.0, 3.5],
        generation: 7,
        shape: Shape::Box { half: [1.0, 2.0, 3.0] },
        tint: [0.1, 0.2, 0.3, 0.4],
        layers: vec![0, 5, 31],
        texture: Some("tex/brick.png".into()),
        label: "crate".into(),
    };
    assert_eq!(Body::from_value(&custom.to_value()), Ok(custom.clone()));

    // Every enum variant.
    for s in [Shape::Sphere { radius: 3.0 }, Shape::Box { half: [1.0; 3] }, Shape::Point] {
        assert_eq!(Shape::from_value(&s.to_value()), Ok(s));
    }
}

#[test]
fn from_value_defaults_missing_and_rejects_bad_input() {
    // Missing fields take defaults.
    let v = Value::Map(vec![("mass".into(), f(2.0))]);
    let b = Body::from_value(&v).unwrap();
    assert_eq!(b, Body { mass: Some(2.0), ..Body::default() });

    // Unknown key: error with suggestion.
    let v = Value::Map(vec![("massa".into(), f(2.0))]);
    assert!(matches!(
        Body::from_value(&v),
        Err(ReflectError::UnknownField { ty: "Body", suggestions, .. }) if suggestions == vec!["mass"]
    ));

    // Nested range violation reports the full path.
    let shape = Value::Enum { variant: "Sphere".into(), fields: vec![("radius".into(), f(-1.0))] };
    let v = Value::Map(vec![("shape".into(), shape)]);
    assert_eq!(Body::from_value(&v), Err(ReflectError::OutOfRange { path: "shape.radius".into(), value: -1.0, min: 0.001, max: 1000.0 }));

    // Not a map.
    assert!(matches!(Body::from_value(&f(1.0)), Err(ReflectError::WrongType { expected: "map", .. })));

    // Whole-value set through the empty path validates too.
    let mut b = Body::default();
    assert!(b.set("", Value::Map(vec![("bounciness".into(), f(9.0))])).is_err());
    b.set("", Value::Map(vec![("bounciness".into(), f(0.9))])).unwrap();
    assert_eq!(b.bounciness, 0.9);
}

#[test]
fn metadata_is_reachable() {
    let info = Body::type_info();
    assert_eq!(info.name, "Body");
    assert_eq!(info.doc, "Rigid body simulated by physics.");
    let mass = info.field("mass").unwrap();
    assert_eq!(mass.range, Some((0.001, 1.0e6)));
    assert_eq!(mass.unit.map(|u| u.symbol()), Some("kg"));
    assert_eq!((mass.default)(), Value::None);
    let vel = info.field("velocity").unwrap();
    assert_eq!(vel.lua, Some("velocity"));
    assert!(info.field("nope").is_none());
    assert_eq!(<Shape as ReflectEnum>::enum_info().variants.len(), 3);
}

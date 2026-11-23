//! The same types as `common::hand`, but derived: metadata, set/get results and round trips must be identical.
#![cfg(feature = "derive")]

mod common;

use common::hand;
use spark_reflect::{Reflect, ReflectEnum, ReflectError, Value};

/// How the body moves.
#[derive(Clone, Copy, Debug, PartialEq, ReflectEnum)]
pub enum BodyKind {
    Static,
    /// Moved by forces.
    Dynamic,
    Kinematic,
}

/// Collision shape.
#[derive(Clone, Debug, PartialEq, ReflectEnum)]
pub enum Shape {
    /// A ball.
    Sphere {
        /// Radius.
        #[reflect(range = 0.001..=1000.0, unit = "m", default = 0.5)]
        radius: f32,
    },
    Box {
        /// Half extents.
        #[reflect(range = 0.001..=1000.0, unit = "m", default = [0.5, 0.5, 0.5])]
        half: [f32; 3],
    },
    Point,
}

/// Rigid body simulated by physics.
#[derive(Clone, Debug, PartialEq, Reflect)]
pub struct Body {
    /// How the body moves.
    pub kind: BodyKind,
    /// Total mass; `None` = computed from volume.
    #[reflect(range = 0.001..=1.0e6, unit = "kg")]
    pub mass: Option<f32>,
    /// Restitution.
    #[reflect(range = 0.0..=1.0, step = 0.01)]
    pub bounciness: f32,
    /// Surface roughness (clamped).
    #[reflect(range = 0.0..=1.0, clamp)]
    pub roughness: f32,
    /// World-space velocity.
    #[reflect(unit = "m/s", transient, lua = "velocity")]
    pub velocity: [f32; 3],
    /// Bumped by the simulation.
    #[reflect(readonly, hidden)]
    pub generation: u32,
    /// Collision shape.
    pub shape: Shape,
    /// Tint (linear RGBA).
    #[reflect(color)]
    pub tint: [f32; 4],
    /// Collision layers.
    #[reflect(range = 0..=31)]
    pub layers: Vec<i32>,
    /// Optional texture.
    #[reflect(asset = "texture")]
    pub texture: Option<String>,
    /// Editor-only label.
    #[reflect(no_lua)]
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

#[test]
fn metadata_is_identical_to_the_hand_impl() {
    assert_eq!(common::describe::<Body>(), common::describe::<hand::Body>());
    assert_eq!(format!("{:#?}", <Shape as ReflectEnum>::enum_info()), format!("{:#?}", <hand::Shape as ReflectEnum>::enum_info()));
    assert_eq!(format!("{:#?}", <BodyKind as ReflectEnum>::enum_info()), format!("{:#?}", <hand::BodyKind as ReflectEnum>::enum_info()));
}

#[test]
fn operations_give_identical_results() {
    let ops = common::ops();
    let (derived_results, derived_final) = common::replay::<Body>(&ops);
    let (hand_results, hand_final) = common::replay::<hand::Body>(&ops);
    assert_eq!(derived_results, hand_results);
    assert_eq!(derived_final, hand_final);
    // Sanity: the list exercises both successes and every error kind.
    let oks = derived_results.iter().filter(|r| r.is_ok()).count();
    assert!(oks > 10 && oks < ops.len());
    for code in ["unknown_field", "wrong_type", "out_of_range", "not_finite", "read_only"] {
        assert!(derived_results.iter().any(|r| matches!(r, Err(e) if e.code() == code)), "no `{code}` error in the operation list");
    }
}

#[test]
fn default_values_and_roundtrip() {
    assert_eq!(Body::default().to_value(), hand::Body::default().to_value());
    let b = Body::default();
    assert_eq!(Body::from_value(&b.to_value()), Ok(b));

    let custom = Body {
        kind: BodyKind::Kinematic,
        mass: Some(12.5),
        shape: Shape::Box { half: [1.0, 2.0, 3.0] },
        tint: [0.1, 0.2, 0.3, 0.4],
        layers: vec![0, 5, 31],
        texture: Some("tex/brick.png".into()),
        label: "crate".into(),
        generation: 9,
        ..Body::default()
    };
    assert_eq!(Body::from_value(&custom.to_value()), Ok(custom.clone()));
    // Same Value as the hand impl produces for the same data.
    let hand_custom = hand::Body::from_value(&custom.to_value()).unwrap();
    assert_eq!(hand_custom.to_value(), custom.to_value());
}

#[test]
fn derived_paths_work() {
    let mut b = Body::default();
    b.set("shape.radius", Value::Float(2.0)).unwrap();
    assert_eq!(b.get("shape.radius"), Ok(Value::Float(2.0)));
    assert!(matches!(b.set("shape.radius", Value::Float(f64::NAN)), Err(ReflectError::NotFinite { .. })));
    assert!(matches!(b.set("bounciness", Value::Float(2.0)), Err(ReflectError::OutOfRange { .. })));
    b.set("roughness", Value::Float(2.0)).unwrap();
    assert_eq!(b.roughness, 1.0);
    let err = b.set("bouncines", Value::Float(0.1)).unwrap_err();
    assert_eq!(err.to_string(), "unknown field `bouncines` on Body; did you mean `bounciness`?");
}

#[test]
fn doc_comments_become_docs() {
    let info = Body::type_info();
    assert_eq!(info.doc, "Rigid body simulated by physics.");
    assert!(info.fields.iter().all(|f| !f.doc.is_empty()));
    assert_eq!(<BodyKind as ReflectEnum>::enum_info().variants[1].doc, "Moved by forces.");
}

/// Multi-line docs, a struct-level `default`, renamed variants, and nesting of reflected structs.
#[derive(Clone, Debug, PartialEq, ReflectEnum)]
enum Mode {
    #[reflect(name = "fast")]
    Fast,
    #[reflect(name = "slow")]
    Slow,
}

/// Inner struct.
///
/// Second line.
#[derive(Clone, Debug, PartialEq, Reflect)]
struct Inner {
    /// Speed.
    #[reflect(range = -1.0..=1.0, default = 0.25)]
    speed: f64,
    /// Mode.
    #[reflect(default = Mode::Slow)]
    mode: Mode,
}

#[derive(Clone, Debug, PartialEq, Reflect)]
struct Outer {
    /// Nested.
    #[reflect(default = Inner { speed: 0.5, mode: Mode::Fast })]
    inner: Inner,
    /// Many.
    #[reflect(default = Vec::new())]
    items: Vec<Inner>,
    /// Target.
    #[reflect(node_ref, default = None)]
    target: Option<String>,
}

impl Default for Outer {
    fn default() -> Self {
        Outer { inner: Inner { speed: 0.5, mode: Mode::Fast }, items: Vec::new(), target: None }
    }
}
impl Default for Inner {
    fn default() -> Self {
        Inner { speed: 0.25, mode: Mode::Slow }
    }
}

#[test]
fn nesting_renames_and_multiline_docs() {
    assert_eq!(Inner::type_info().doc, "Inner struct.\n\nSecond line.");
    let mut o = Outer::default();
    o.set("inner.speed", Value::Float(0.75)).unwrap();
    o.set("inner.mode", Value::Str("slow".into())).unwrap();
    assert_eq!(o.inner, Inner { speed: 0.75, mode: Mode::Slow });
    assert_eq!(
        o.set("inner.speed", Value::Float(2.0)),
        Err(ReflectError::OutOfRange { path: "inner.speed".into(), value: 2.0, min: -1.0, max: 1.0 })
    );
    assert!(matches!(
        o.set("inner.mode", Value::Str("slw".into())),
        Err(ReflectError::UnknownField { suggestions, .. }) if suggestions == vec!["slow"]
    ));
    o.set("target", Value::Str("node-1".into())).unwrap();
    assert_eq!(o.get("target"), Ok(Value::NodeRef("node-1".into())));
    o.items.push(Inner::default());
    assert_eq!(Outer::from_value(&o.to_value()), Ok(o.clone()));
    // Errors inside a list element carry the index.
    let bad = Value::Map(vec![("items".into(), Value::List(vec![Value::Map(vec![("speed".into(), Value::Float(5.0))])]))]);
    assert_eq!(Outer::from_value(&bad), Err(ReflectError::OutOfRange { path: "items[0].speed".into(), value: 5.0, min: -1.0, max: 1.0 }));
}

//! The reflected engine data (ADR 0004, step C-3): defaults round-trip, every field is documented, the declared ranges
//! are the limits the runtime enforces today (scripting API: `spark-script/src/{types,physics,light,api}.rs`,
//! `Material::with_*`), and the Luau names are the current ones.

use spark_core::material::BlendMode;
use spark_core::{
    Body, BodyKind, Camera, Color, Fog, Light, LightKind, Material, MeshId, Quat, Shape, Sky, Sun, TextureId, Transform, Vec3,
};
use spark_reflect::{EnumInfo, FieldFlags, FieldInfo, Reflect, ReflectEnum, ReflectError, TypeInfo, Value};

fn f(x: f32) -> Value {
    Value::Float(f64::from(x))
}

/// Equal, with float tolerance (a quaternion goes through Euler angles).
fn close(a: &Value, b: &Value) -> bool {
    let near = |x: f32, y: f32| (x - y).abs() <= 1e-3 * (1.0 + x.abs().max(y.abs()));
    match (a, b) {
        (Value::Float(x), Value::Float(y)) => near(*x as f32, *y as f32),
        (Value::Vec2(x), Value::Vec2(y)) => x.iter().zip(y).all(|(p, q)| near(*p, *q)),
        (Value::Vec3(x), Value::Vec3(y)) => x.iter().zip(y).all(|(p, q)| near(*p, *q)),
        (Value::Vec4(x), Value::Vec4(y)) | (Value::Color(x), Value::Color(y)) => x.iter().zip(y).all(|(p, q)| near(*p, *q)),
        (Value::Map(x), Value::Map(y)) => x.len() == y.len() && x.iter().zip(y).all(|((k, p), (l, q))| k == l && close(p, q)),
        (Value::Enum { variant: v, fields: x }, Value::Enum { variant: w, fields: y }) => {
            v == w && x.len() == y.len() && x.iter().zip(y).all(|((k, p), (l, q))| k == l && close(p, q))
        }
        (Value::List(x), Value::List(y)) => x.len() == y.len() && x.iter().zip(y).all(|(p, q)| close(p, q)),
        _ => a == b,
    }
}

fn roundtrip<T: Reflect + Clone + std::fmt::Debug>(x: &T) {
    let v = x.to_value();
    let back = T::from_value(&v).unwrap_or_else(|e| panic!("{}: {e}", T::type_info().name));
    // Equal as values (a `Quat` comes back as the same angles, not necessarily the same bits).
    assert!(close(&back.to_value(), &v), "{}: {:?} vs {:?}", T::type_info().name, back.to_value(), v);
}

#[test]
fn defaults_round_trip() {
    // Exactly equal (identity rotation and plain numbers survive unchanged).
    macro_rules! exact {
        ($($t:ty),*) => {$(
            let d = <$t>::default();
            assert_eq!(<$t as Reflect>::from_value(&d.to_value()), Ok(d), "{}", stringify!($t));
        )*};
    }
    exact!(Transform, Material, Body, Light, Sun, Fog, Sky);

    // The camera looks at the origin: its rotation is a quaternion that comes back through Euler angles.
    let cam = Camera::default();
    let back = Camera::from_value(&cam.to_value()).unwrap();
    assert_eq!((back.position, back.fov, back.near, back.far, back.ortho), (cam.position, cam.fov, cam.near, cam.far, cam.ortho));
    assert!(back.rotation.angle_between(cam.rotation) < 1e-4, "{:?} vs {:?}", back.rotation, cam.rotation);
}

#[test]
fn non_default_values_round_trip() {
    roundtrip(&Transform { position: Vec3::new(1.0, -2.0, 3.5), rotation: Quat::from_rotation_y(0.7), scale: Vec3::new(2.0, 1.0, 0.5) });
    roundtrip(&Material::pbr(Color::rgba(0.2, 0.4, 0.6, 0.5), 0.8, 0.1).with_blend(BlendMode::Additive).with_emissive(Color::RED));
    roundtrip(&Body { mass: Some(12.5), shape: Shape::Capsule { radius: 0.3, height: 1.8 }, kind: BodyKind::Kinematic, ..Body::default() });
    roundtrip(&Light::spot(Color::YELLOW, 3.0, 12.0, 0.9, 0.2));
    roundtrip(&Camera { ortho: Some(10.0), fov: 90.0, ..Camera::default() });
    roundtrip(&Sun { direction: Vec3::new(0.0, -1.0, 0.0), color: Color::ORANGE, intensity: 2.5 });
    roundtrip(&Fog { color: Color::SKY, near: 1.0, far: 99.0 });
    roundtrip(&Sky { top: Color::BLUE, bottom: Color::BLACK, cubemap: None, blend: 0.5, lighting: 1.5 });

    for m in [BlendMode::Auto, BlendMode::Opaque, BlendMode::Alpha, BlendMode::Additive] {
        assert_eq!(BlendMode::from_value(&m.to_value()), Ok(m));
        assert_eq!(m.variant_name(), m.name(), "the reflected name is the Luau name");
    }
    for k in [BodyKind::Dynamic, BodyKind::Static, BodyKind::Kinematic] {
        assert_eq!(BodyKind::from_value(&k.to_value()), Ok(k));
        assert_eq!(k.variant_name(), k.name());
    }
    for s in [
        Shape::Auto,
        Shape::Box { size: Vec3::new(1.0, 2.0, 3.0) },
        Shape::Sphere { radius: 2.0 },
        Shape::Capsule { radius: 0.4, height: 1.7 },
        Shape::Cylinder { radius: 0.6, height: 3.0 },
    ] {
        assert_eq!(Shape::from_value(&s.to_value()), Ok(s));
    }
    for l in [LightKind::Point, LightKind::Spot { angle: 0.5, softness: 0.25 }] {
        assert_eq!(LightKind::from_value(&l.to_value()), Ok(l));
    }
    // Names that scripts already use as strings.
    assert_eq!(BlendMode::from_value(&Value::Str("additive".into())), Ok(BlendMode::Additive));
    assert_eq!(BodyKind::from_value(&Value::Str("static".into())), Ok(BodyKind::Static));
    assert_eq!(Shape::from_value(&Value::Str("auto".into())), Ok(Shape::Auto));
    assert_eq!(LightKind::from_value(&Value::Str("point".into())), Ok(LightKind::Point));
}

// ---------------------------------------------------------------------------------------------------------
// Documentation lint

fn lint_fields(owner: &str, fields: &[FieldInfo]) {
    for fi in fields {
        assert!(!fi.doc.trim().is_empty(), "{owner}.{} has no doc comment", fi.name);
    }
}

fn lint_struct(info: &TypeInfo) {
    assert!(!info.doc.trim().is_empty(), "{} has no doc comment", info.name);
    lint_fields(info.name, info.fields);
}

fn lint_enum(info: &EnumInfo) {
    assert!(!info.doc.trim().is_empty(), "{} has no doc comment", info.name);
    for v in info.variants {
        assert!(!v.doc.trim().is_empty(), "{}::{} has no doc comment", info.name, v.name);
        lint_fields(&format!("{}::{}", info.name, v.name), v.fields);
    }
}

#[test]
fn every_type_field_and_variant_has_a_doc() {
    for info in [
        Transform::type_info(),
        Material::type_info(),
        Body::type_info(),
        Light::type_info(),
        Camera::type_info(),
        Sun::type_info(),
        Fog::type_info(),
        Sky::type_info(),
    ] {
        lint_struct(info);
    }
    for info in [BlendMode::enum_info(), BodyKind::enum_info(), Shape::enum_info(), LightKind::enum_info()] {
        lint_enum(info);
    }
}

// ---------------------------------------------------------------------------------------------------------
// Ranges = the limits the runtime enforces today

fn range_of(info: &'static TypeInfo, name: &str) -> Option<(f64, f64)> {
    info.field(name).unwrap_or_else(|| panic!("{} has no field {name}", info.name)).range
}

fn clamps(info: &'static TypeInfo, name: &str) -> bool {
    info.field(name).unwrap().flags.contains(FieldFlags::CLAMP)
}

fn is_out_of_range(r: Result<(), ReflectError>) -> bool {
    matches!(r, Err(ReflectError::OutOfRange { .. }))
}

const MAX: f64 = f32::MAX as f64;

#[test]
fn material_ranges_are_the_with_metallic_and_with_roughness_clamps() {
    for name in ["metallic", "roughness"] {
        assert_eq!(range_of(Material::type_info(), name), Some((0.0, 1.0)));
        assert!(clamps(Material::type_info(), name), "{name} is clamped, not rejected");
    }
    for x in [-5.0_f32, -0.1, 0.0, 0.3, 1.0, 7.0] {
        let mut m = Material::default();
        m.set("metallic", f(x)).unwrap();
        m.set("roughness", f(x)).unwrap();
        let expected = Material::default().with_metallic(x).with_roughness(x);
        assert_eq!((m.metallic, m.roughness), (expected.metallic, expected.roughness), "value {x}");
    }
    // Everything else on the material is unbounded (finite numbers only).
    for name in ["color", "tiling", "data", "emissive"] {
        assert_eq!(range_of(Material::type_info(), name), None, "{name}");
    }
}

#[test]
fn camera_ranges_are_the_script_checks() {
    let info = Camera::type_info();
    assert_eq!(range_of(info, "fov"), Some((1.0, 179.0)));
    assert!(!clamps(info, "fov"), "`camera.fov = 0` is an error in scripts");
    let mut c = Camera::default();
    assert!(is_out_of_range(c.set("fov", f(0.5))));
    assert!(is_out_of_range(c.set("fov", f(179.5))));
    c.set("fov", f(1.0)).unwrap();
    c.set("fov", f(179.0)).unwrap();

    // `near` / `far`: strictly positive (the cross check near < far stays with the script API).
    for name in ["near", "far"] {
        let (lo, hi) = range_of(info, name).unwrap();
        assert!(lo > 0.0 && lo < 1e-44, "{name}: smallest positive f32, got {lo}");
        assert_eq!(hi, MAX);
        assert!(!clamps(info, name));
        assert!(is_out_of_range(c.set(name, f(0.0))), "{name} = 0");
        assert!(is_out_of_range(c.set(name, f(-1.0))), "{name} = -1");
        c.set(name, f(f32::MIN_POSITIVE)).unwrap();
    }

    // `camera.ortho = h` is clamped to at least 0.001; nil / false (None) is perspective.
    assert_eq!(range_of(info, "ortho"), Some((0.001, MAX)));
    assert!(clamps(info, "ortho"));
    c.set("ortho", f(0.0)).unwrap();
    assert_eq!(c.ortho, Some(0.001));
    c.set("ortho", Value::None).unwrap();
    assert_eq!(c.ortho, None);
}

#[test]
fn body_ranges_are_the_script_checks() {
    let info = Body::type_info();
    // mass: `positive`; `None` stays valid.
    let (lo, hi) = range_of(info, "mass").unwrap();
    assert!(lo > 0.0 && lo < 1e-44 && hi == MAX);
    let mut b = Body::default();
    assert!(is_out_of_range(b.set("mass", f(0.0))));
    assert!(is_out_of_range(b.set("mass", f(-2.0))));
    b.set("mass", f(1e-30)).unwrap();
    assert_eq!(b.mass, Some(1e-30));
    b.set("mass", Value::None).unwrap();
    assert_eq!(b.mass, None);
    // friction: `non_negative`.
    assert_eq!(range_of(info, "friction"), Some((0.0, MAX)));
    assert!(is_out_of_range(b.set("friction", f(-0.1))));
    b.set("friction", f(0.0)).unwrap();
    // No script check: bounciness, gravity_scale, the dampings, the vectors.
    for name in ["bounciness", "gravity_scale", "linear_damping", "angular_damping", "velocity", "angular_velocity"] {
        assert_eq!(range_of(info, name), None, "{name}");
    }
    // shape sizes: `positive` radius and height; the box size is unchecked.
    let ranges: Vec<(&str, &str)> =
        vec![("sphere", "radius"), ("capsule", "radius"), ("capsule", "height"), ("cylinder", "radius"), ("cylinder", "height")];
    for (variant, field) in ranges {
        let v = Shape::enum_info().variant(variant).unwrap().fields.iter().find(|x| x.name == field).unwrap();
        let (lo, hi) = v.range.unwrap();
        assert!(lo > 0.0 && lo < 1e-44 && hi == MAX, "{variant}.{field}");
    }
    assert_eq!(Shape::enum_info().variant("box").unwrap().fields[0].range, None);
    let mut s = Shape::Sphere { radius: 1.0 };
    assert!(is_out_of_range(s.set("radius", f(0.0))));
    s.set("radius", f(0.25)).unwrap();
    assert_eq!(s, Shape::Sphere { radius: 0.25 });
}

#[test]
fn light_ranges_are_the_script_clamps() {
    let info = Light::type_info();
    assert_eq!(range_of(info, "intensity"), Some((0.0, MAX)));
    assert_eq!(range_of(info, "range"), Some((0.01, MAX)));
    assert!(clamps(info, "intensity") && clamps(info, "range"));
    let mut l = Light::spot(Color::WHITE, 1.0, 10.0, 0.5, 0.5);
    l.set("intensity", f(-3.0)).unwrap();
    assert_eq!(l.intensity, 0.0, "`.max(0.0)`");
    l.set("range", f(0.001)).unwrap();
    assert_eq!(l.range, 0.01_f32, "`.max(0.01)`");

    // Spot: the script gives degrees, clamps them to 1..179 and stores radians.
    let spot = LightKind::enum_info().variant("spot").unwrap();
    let angle = spot.fields.iter().find(|x| x.name == "angle").unwrap();
    assert_eq!(angle.range, Some((f64::from(1.0_f32.to_radians()), f64::from(179.0_f32.to_radians()))));
    assert!(angle.flags.contains(FieldFlags::CLAMP));
    let softness = spot.fields.iter().find(|x| x.name == "softness").unwrap();
    assert_eq!(softness.range, Some((0.0, 1.0)));
    assert!(softness.flags.contains(FieldFlags::CLAMP));
    for degrees in [0.5_f32, 1.0, 45.0, 179.0, 200.0] {
        let mut l = Light::spot(Color::WHITE, 1.0, 10.0, 0.5, 0.5);
        l.set("kind.angle", f(degrees.to_radians())).unwrap();
        let LightKind::Spot { angle, .. } = l.kind else { panic!() };
        assert_eq!(angle, degrees.clamp(1.0, 179.0).to_radians(), "{degrees} degrees");
    }
    for s in [-1.0_f32, 0.0, 0.4, 1.0, 3.0] {
        let mut l = Light::spot(Color::WHITE, 1.0, 10.0, 0.5, 0.5);
        l.set("kind.softness", f(s)).unwrap();
        let LightKind::Spot { softness, .. } = l.kind else { panic!() };
        assert_eq!(softness, s.clamp(0.0, 1.0));
    }
    // A point light has no angle.
    assert!(Light::point(Color::WHITE, 1.0, 5.0).set("kind.angle", f(1.0)).is_err());
}

#[test]
fn sky_ranges_are_the_script_clamps() {
    let info = Sky::type_info();
    assert_eq!((range_of(info, "blend"), clamps(info, "blend")), (Some((0.0, 1.0)), true));
    assert_eq!((range_of(info, "lighting"), clamps(info, "lighting")), (Some((0.0, MAX)), true));
    let mut sky = Sky::default();
    sky.set("blend", f(5.0)).unwrap();
    sky.set("lighting", f(-1.0)).unwrap();
    assert_eq!((sky.blend, sky.lighting), (1.0, 0.0));
    assert_eq!(Sky::default().with_lighting(-1.0).lighting, 0.0, "same as `Sky::with_lighting`");
}

#[test]
fn non_finite_numbers_are_rejected_everywhere() {
    let mut m = Material::default();
    for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(matches!(m.set("metallic", Value::Float(bad)), Err(ReflectError::NotFinite { .. })));
        assert!(matches!(Body::default().set("bounciness", Value::Float(bad)), Err(ReflectError::NotFinite { .. })));
    }
    assert!(matches!(Transform::default().set("position.x", Value::Float(f64::NAN)), Err(ReflectError::NotFinite { .. })));
}

// ---------------------------------------------------------------------------------------------------------
// Luau names, handles, rotation

fn lua_names(info: &'static TypeInfo) -> Vec<(&'static str, &'static str)> {
    info.fields.iter().filter_map(|fi| fi.lua.map(|l| (fi.name, l))).collect()
}

#[test]
fn lua_names_are_the_current_luau_names() {
    // Flat on `Object`.
    assert_eq!(lua_names(Transform::type_info()), [("position", "position"), ("rotation", "rotation"), ("scale", "scale")]);
    assert_eq!(
        lua_names(Material::type_info()),
        [
            ("color", "color"),
            ("shader", "shader"),
            ("data", "data"),
            ("blend", "blend"),
            ("double_sided", "double_sided"),
            ("cast_shadow", "cast_shadow"),
            ("pbr", "pbr"),
            ("metallic", "metallic"),
            ("roughness", "roughness"),
            ("emissive", "emissive"),
            ("normal_map", "normal_map"),
            ("orm_map", "orm_map"),
        ]
    );
    assert_eq!(
        lua_names(Body::type_info()),
        [
            ("mass", "mass"),
            ("sensor", "sensor"),
            ("gravity_scale", "gravity_scale"),
            ("velocity", "velocity"),
            ("angular_velocity", "angular_velocity")
        ]
    );
    // Members of `obj.light` (not flat on `Object`: `obj.color` is the material color).
    assert_eq!(
        lua_names(Light::type_info()),
        [("kind", "type"), ("color", "color"), ("intensity", "intensity"), ("range", "range"), ("shadows", "shadows")]
    );
    let spot = LightKind::enum_info().variant("spot").unwrap();
    let spot_names: Vec<_> = spot.fields.iter().filter_map(|fi| fi.lua).collect();
    assert_eq!(spot_names, ["angle", "softness"]);
    // Members of the global `camera`.
    assert_eq!(
        lua_names(Camera::type_info()),
        [("position", "position"), ("rotation", "rotation"), ("fov", "fov"), ("near", "near"), ("far", "far"), ("ortho", "ortho")]
    );

    // No two flat names collide on `Object` (transform + material + body).
    let mut flat: Vec<&str> = [Transform::type_info(), Material::type_info(), Body::type_info()]
        .iter()
        .flat_map(|t| t.fields.iter().filter_map(|fi| fi.lua))
        .collect();
    flat.sort_unstable();
    let n = flat.len();
    flat.dedup();
    assert_eq!(flat.len(), n, "a flat Luau name is used twice");
}

#[test]
fn handle_fields_are_runtime_only() {
    let handles = [
        (Material::type_info(), "texture"),
        (Material::type_info(), "shader"),
        (Material::type_info(), "normal_map"),
        (Material::type_info(), "orm_map"),
        (Sky::type_info(), "cubemap"),
    ];
    for (info, name) in handles {
        let flags = info.field(name).unwrap().flags;
        assert!(flags.contains(FieldFlags::READ_ONLY) && flags.contains(FieldFlags::TRANSIENT), "{}.{name}: {flags:?}", info.name);
    }
    // A handle is shown, but cannot be written through the registry.
    let mut m = Material::textured(TextureId(4));
    assert_eq!(m.get("texture"), Ok(Value::Asset("runtime:texture/4".into())));
    assert!(matches!(m.set("texture", Value::None), Err(ReflectError::ReadOnly { .. })));
    assert_eq!(m.texture, Some(TextureId(4)));
    assert!(Material::from_value(&m.to_value()).is_err(), "no scene-file form for a handle yet");
    let _ = MeshId(0);
}

#[test]
fn rotation_is_euler_degrees() {
    assert_eq!(Transform::type_info().field("rotation").unwrap().unit.map(|u| u.symbol()), Some("deg"));
    let mut t = Transform::default();
    t.set("rotation", Value::Vec3([0.0, 90.0, 0.0])).unwrap();
    // 90 degrees of yaw turns -Z into -X.
    let forward = t.rotation * Vec3::NEG_Z;
    assert!((forward - Vec3::NEG_X).length() < 1e-5, "{forward}");
    let Ok(Value::Vec3(e)) = t.get("rotation") else { panic!() };
    assert!((e[1] - 90.0).abs() < 1e-3);
    t.set("rotation.x", Value::Float(30.0)).unwrap();
    let Ok(Value::Float(pitch)) = t.get("rotation.x") else { panic!() };
    assert!((pitch - 30.0).abs() < 1e-3);
}

//! The component registry over `Object` (ADR 0004/0005, step C-5): every field can be set and read back through
//! `Object::set_prop` / `get_prop` and through `ComponentInfo`, components can be added and removed, errors name the
//! path. (The exhaustive destructuring of `Object` lives next to the registry, in `src/registry.rs`: it needs the
//! crate-private fields.)

use spark_core::registry::{FieldDecision, LuaScope, OBJECT_FIELDS, registry};
use spark_core::{Body, BodyKind, Color, Light, LightKind, Material, MeshId, Object, Quat, Shape, Transform, Vec3};
use spark_reflect::{FieldFlags, FieldInfo, FieldType, Reflect, ReflectError, Value};

/// Equal, with float tolerance (rotations go through Euler angles).
fn close(a: &Value, b: &Value) -> bool {
    let near = |x: f32, y: f32| (x - y).abs() <= 1e-3 * (1.0 + x.abs().max(y.abs()));
    match (a, b) {
        (Value::Float(x), Value::Float(y)) => near(*x as f32, *y as f32),
        (Value::Vec2(x), Value::Vec2(y)) => x.iter().zip(y).all(|(p, q)| near(*p, *q)),
        (Value::Vec3(x), Value::Vec3(y)) => x.iter().zip(y).all(|(p, q)| near(*p, *q)),
        (Value::Vec4(x), Value::Vec4(y)) | (Value::Color(x), Value::Color(y)) => x.iter().zip(y).all(|(p, q)| near(*p, *q)),
        _ => a == b,
    }
}

/// An object with every component present.
fn full_object() -> Object {
    let mut o = Object::empty();
    o.mesh = Some(MeshId(7));
    o.lod = vec![(20.0, MeshId(8))];
    for name in ["body", "light"] {
        (registry().component(name).unwrap().add)(&mut o, None).unwrap();
    }
    o
}

/// A valid value for a field that differs from its default where possible. `None` = not settable through a value
/// (lists, assets, node references; enums are covered separately).
fn sample(info: &FieldInfo, ty: &FieldType) -> Option<Value> {
    Some(match ty {
        FieldType::Bool => Value::Bool(!matches!((info.default)(), Value::Bool(true))),
        FieldType::Int => Value::Int(3),
        FieldType::Float => {
            let x = match info.range {
                Some((lo, hi)) => (lo.max(-1.0e6) + hi.min(1.0e6)) / 2.0,
                None => 2.5,
            };
            Value::Float(f64::from(x as f32))
        }
        FieldType::Str => Value::Str("sample".into()),
        FieldType::Vec2 => Value::Vec2([1.5, 2.5]),
        FieldType::Vec3 => Value::Vec3([1.5, 2.5, 3.5]),
        FieldType::Vec4 => Value::Vec4([0.5, 1.5, 2.5, 3.5]),
        FieldType::Color => Value::Color([0.25, 0.5, 0.75, 1.0]),
        FieldType::Option(inner) => return sample(info, inner),
        FieldType::Enum(_) | FieldType::Struct(_) | FieldType::List(_) | FieldType::NodeRef | FieldType::Asset(_) => return None,
    })
}

#[test]
fn the_registry_lists_the_components() {
    let names: Vec<&str> = registry().components.iter().map(|c| c.name).collect();
    assert_eq!(names, ["transform", "mesh", "material", "body", "light"]);
    let required: Vec<&str> = registry().components.iter().filter(|c| c.required).map(|c| c.name).collect();
    assert_eq!(required, ["transform", "material"]);
    let types: Vec<&str> = registry().components.iter().map(|c| c.info().name).collect();
    assert_eq!(types, ["Transform", "MeshRef", "Material", "Body", "Light"]);
    assert_eq!(registry().node_info().name, "NodeProps");
    assert_eq!(registry().component("light").unwrap().lua, LuaScope::Handle("light"));
    assert_eq!(registry().component("body").unwrap().lua, LuaScope::Object);
    assert!(registry().component("camera").is_none());
}

#[test]
fn every_field_sets_then_gets_back() {
    let mut covered = 0;
    for c in registry().components {
        for fi in c.info().fields {
            let path = format!("{}.{}", c.name, fi.name);
            let mut o = full_object();
            if fi.flags.contains(FieldFlags::READ_ONLY) {
                // Runtime handles: shown, never written.
                let before = o.get_prop(&path).unwrap();
                let err = o.set_prop(&path, before.clone()).unwrap_err();
                assert!(matches!(err, ReflectError::ReadOnly { .. }), "{path}: {err:?}");
                assert_eq!(err.path(), path);
                assert_eq!(o.get_prop(&path), Ok(before), "{path}: a refused write changes nothing");
                continue;
            }
            let Some(v) = sample(fi, &fi.ty) else { continue };
            // Through the object ...
            o.set_prop(&path, v.clone()).unwrap_or_else(|e| panic!("set {path} = {v:?}: {e}"));
            let got = o.get_prop(&path).unwrap();
            assert!(close(&got, &v), "{path}: set {v:?}, got {got:?}");
            // ... and through the component accessors.
            let mut p = full_object();
            (c.set)(&mut p, fi.name, v.clone()).unwrap();
            let got = (c.get_path)(&p, fi.name).unwrap();
            assert!(close(&got, &v), "{path} (component): set {v:?}, got {got:?}");
            // The whole component stays valid after the write.
            let whole = (c.get)(&p).unwrap();
            assert!(matches!(whole, Value::Map(_)), "{path}");
            covered += 1;
        }
    }
    assert!(covered > 30, "only {covered} fields were exercised");
}

#[test]
fn sub_paths_and_enum_variants_round_trip() {
    let mut o = full_object();
    o.set_prop("transform.position.y", Value::Float(4.0)).unwrap();
    assert_eq!(o.transform.position, Vec3::new(0.0, 4.0, 0.0));
    assert_eq!(o.get_prop("transform.position"), Ok(Value::Vec3([0.0, 4.0, 0.0])));
    o.set_prop("transform.rotation", Value::Vec3([0.0, 90.0, 0.0])).unwrap();
    assert!((o.transform.rotation * Vec3::NEG_Z - Vec3::NEG_X).length() < 1e-5, "degrees, yaw 90");
    o.set_prop("material.color", Value::Color([1.0, 0.0, 0.0, 1.0])).unwrap();
    assert_eq!(o.material.color, Color::rgb(1.0, 0.0, 0.0));
    o.set_prop("material.blend", Value::Str("additive".into())).unwrap();
    assert_eq!(o.material.blend, spark_core::material::BlendMode::Additive);

    // Body.kind: every variant by name; Body.shape: every variant, then a field of it.
    for (name, kind) in [("static", BodyKind::Static), ("kinematic", BodyKind::Kinematic), ("dynamic", BodyKind::Dynamic)] {
        o.set_prop("body.kind", Value::unit(name)).unwrap();
        assert_eq!(o.body.as_ref().unwrap().kind, kind);
        assert_eq!(o.get_prop("body.kind"), Ok(Value::unit(name)));
    }
    for name in ["auto", "box", "sphere", "capsule", "cylinder"] {
        o.set_prop("body.shape", Value::unit(name)).unwrap();
        assert_eq!(o.get_prop("body.shape").map(|v| matches!(v, Value::Enum { variant, .. } if variant == name)), Ok(true), "{name}");
    }
    o.set_prop("body.shape", Value::unit("sphere")).unwrap();
    o.set_prop("body.shape.radius", Value::Float(2.0)).unwrap();
    assert_eq!(o.body.as_ref().unwrap().shape, Shape::Sphere { radius: 2.0 });
    assert!(matches!(o.set_prop("body.shape.radius", Value::Float(0.0)), Err(ReflectError::OutOfRange { .. })));
    assert!(o.set_prop("body.shape.size", Value::Vec3([1.0; 3])).is_err(), "a sphere has no size");

    // Light.kind.
    o.set_prop("light.kind", Value::unit("spot")).unwrap();
    assert!(matches!(o.light.unwrap().kind, LightKind::Spot { .. }));
    o.set_prop("light.kind.angle", Value::Float(0.5)).unwrap();
    assert_eq!(o.light.unwrap().kind, LightKind::Spot { angle: 0.5, softness: 0.3 });
    o.set_prop("light.kind", Value::unit("point")).unwrap();
    assert_eq!(o.light.unwrap().kind, LightKind::Point);

    // Node fields.
    o.set_prop("name", Value::Str("crate".into())).unwrap();
    o.set_prop("visible", Value::Bool(false)).unwrap();
    assert_eq!((o.name.as_str(), o.visible), ("crate", false));
    assert_eq!(o.get_prop("name"), Ok(Value::Str("crate".into())));
    assert!(matches!(o.set_prop("visible", Value::Float(1.0)), Err(ReflectError::WrongType { .. })));
}

#[test]
fn registry_errors_name_the_path() {
    let mut o = full_object();
    // Range and clamp, with the full path.
    let err = o.set_prop("body.mass", Value::Float(0.0)).unwrap_err();
    assert!(matches!(err, ReflectError::OutOfRange { .. }));
    assert_eq!(err.path(), "body.mass");
    o.set_prop("material.metallic", Value::Float(3.0)).unwrap();
    assert_eq!(o.material.metallic, 1.0, "clamped like Material::with_metallic");
    assert_eq!(o.set_prop("transform.position.x", Value::Float(f64::NAN)).unwrap_err().path(), "transform.position.x");
    assert!(matches!(o.set_prop("transform.position", Value::Float(1.0)), Err(ReflectError::WrongType { .. })));

    // Did you mean ... (component, then field).
    match o.set_prop("bodyy.mass", Value::Float(1.0)).unwrap_err() {
        ReflectError::UnknownField { ty, suggestions, .. } => assert_eq!((ty, suggestions), ("Object", vec!["body"])),
        other => panic!("{other:?}"),
    }
    match o.get_prop("material.colour").unwrap_err() {
        ReflectError::UnknownField { path, suggestions, .. } => {
            assert_eq!((path.as_str(), suggestions), ("material.colour", vec!["color"]))
        }
        other => panic!("{other:?}"),
    }
    assert!(matches!(o.get_prop("visble"), Err(ReflectError::UnknownField { suggestions, .. }) if suggestions == vec!["visible"]));

    // A missing component: reading the component gives `none`, reaching into it is an error, nothing changes.
    let mut bare = Object::empty();
    assert_eq!(bare.get_prop("body"), Ok(Value::None));
    assert_eq!(bare.get_prop("light"), Ok(Value::None));
    assert_eq!(bare.get_prop("mesh"), Ok(Value::None));
    let err = bare.get_prop("body.mass").unwrap_err();
    assert!(matches!(err, ReflectError::WrongType { got: "none", .. }), "{err:?}");
    assert_eq!(err.path(), "body");
    assert!(bare.set_prop("body.mass", Value::Float(1.0)).is_err());
    assert!(bare.body.is_none());
}

#[test]
fn add_and_remove_body_and_light() {
    let mut o = Object::empty();
    let body = registry().component("body").unwrap();
    let light = registry().component("light").unwrap();
    assert_eq!((body.get)(&o), None);

    // Defaults.
    (body.add)(&mut o, None).unwrap();
    assert_eq!(o.body, Some(Body::default()));
    assert_eq!((body.get)(&o), Some(Body::default().to_value()));
    (light.add)(&mut o, None).unwrap();
    assert_eq!(o.light, Some(Light::default()));

    // With a value; an existing component is replaced.
    let v = Value::Map(vec![("kind".into(), Value::unit("static")), ("mass".into(), Value::Float(5.0))]);
    (body.add)(&mut o, Some(&v)).unwrap();
    let b = o.body.as_ref().unwrap();
    assert_eq!((b.kind, b.mass, b.friction), (BodyKind::Static, Some(5.0), 0.5));
    let spot = Value::Map(vec![("kind".into(), Value::unit("spot")), ("intensity".into(), Value::Float(3.0))]);
    (light.add)(&mut o, Some(&spot)).unwrap();
    assert_eq!(o.light.unwrap().intensity, 3.0);
    assert!(matches!(o.light.unwrap().kind, LightKind::Spot { .. }));

    // Invalid values add nothing and change nothing.
    let bad = Value::Map(vec![("mass".into(), Value::Float(-1.0))]);
    assert!(matches!((body.add)(&mut o, Some(&bad)), Err(ReflectError::OutOfRange { .. })));
    assert_eq!(o.body.as_ref().unwrap().mass, Some(5.0));
    assert!((body.add)(&mut o, Some(&Value::Float(1.0))).is_err());

    // Remove: true once, then false.
    assert!((body.remove)(&mut o));
    assert!(!(body.remove)(&mut o));
    assert!(o.body.is_none());
    assert_eq!((body.get)(&o), None);
    assert!((light.remove)(&mut o));
    assert!(!(light.remove)(&mut o));
    assert!(o.light.is_none());
    // Add again after remove.
    (body.add)(&mut o, None).unwrap();
    assert_eq!(o.body, Some(Body::default()));
}

#[test]
fn required_components_cannot_be_removed_and_the_mesh_is_read_only() {
    let mut o = full_object();
    let transform = registry().component("transform").unwrap();
    let material = registry().component("material").unwrap();
    o.transform.position = Vec3::new(1.0, 2.0, 3.0);
    assert!(!(transform.remove)(&mut o));
    assert!(!(material.remove)(&mut o));
    assert_eq!(o.transform.position, Vec3::new(1.0, 2.0, 3.0));
    // `add` on a required component resets / replaces it.
    (transform.add)(&mut o, None).unwrap();
    assert_eq!(o.transform, Transform::default());
    o.material = Material::color(Color::RED);
    (material.add)(&mut o, None).unwrap();
    assert_eq!(o.material, Material::default());
    o.transform.rotation = Quat::from_rotation_x(0.3);

    // Mesh: visible through the registry, but not created or changed (meshes are runtime handles until C-11).
    let mesh = registry().component("mesh").unwrap();
    let Value::Map(pairs) = (mesh.get)(&o).unwrap() else { panic!() };
    assert_eq!(pairs[0], ("mesh".to_string(), Value::Asset("runtime:mesh/7".into())));
    assert!(matches!(o.get_prop("mesh.lod"), Ok(Value::List(l)) if l.len() == 1));
    assert!(matches!(o.set_prop("mesh.lod", Value::List(vec![])), Err(ReflectError::ReadOnly { .. })));
    assert!(matches!((mesh.add)(&mut o, None), Err(ReflectError::ReadOnly { .. })));
    assert_eq!(o.mesh, Some(MeshId(7)));
    // Removing it is allowed (like `obj.mesh = nil`) and also drops the level-of-detail meshes.
    assert!((mesh.remove)(&mut o));
    assert!(!(mesh.remove)(&mut o));
    assert_eq!((o.mesh, o.lod.len()), (None, 0));
    assert_eq!((mesh.get)(&o), None);
    assert!(matches!((mesh.add)(&mut Object::empty(), None), Err(ReflectError::ReadOnly { .. })));
}

#[test]
fn the_whole_object_is_a_map_of_the_present_parts() {
    let o = full_object();
    let Ok(Value::Map(pairs)) = o.get_prop("") else { panic!() };
    let keys: Vec<&str> = pairs.iter().map(|(k, _)| k.as_str()).collect();
    assert_eq!(keys, ["name", "visible", "transform", "mesh", "material", "body", "light"]);
    let bare = Object::empty();
    let Ok(Value::Map(pairs)) = bare.get_prop("") else { panic!() };
    let keys: Vec<&str> = pairs.iter().map(|(k, _)| k.as_str()).collect();
    assert_eq!(keys, ["name", "visible", "transform", "material"]);
}

#[test]
fn object_fields_table_matches_the_registry() {
    let names: Vec<&str> = registry().components.iter().map(|c| c.name).collect();
    for (field, decision) in OBJECT_FIELDS {
        match decision {
            FieldDecision::Component(c) => assert!(names.contains(c), "`{field}` points at the unknown component `{c}`"),
            FieldDecision::Node => assert!(registry().node_info().field(field).is_some(), "`{field}` is not a node field"),
            FieldDecision::Hierarchy | FieldDecision::Derived | FieldDecision::Runtime => {}
        }
    }
    for name in &names {
        assert!(OBJECT_FIELDS.iter().any(|(_, d)| *d == FieldDecision::Component(name)), "no Object field maps to `{name}`");
    }
    for fi in registry().node_info().fields {
        assert!(
            OBJECT_FIELDS.iter().any(|(n, d)| *n == fi.name && *d == FieldDecision::Node),
            "node field `{}` is not in OBJECT_FIELDS",
            fi.name
        );
    }
}

#[test]
fn flat_lua_names_do_not_collide_and_every_field_is_documented() {
    let mut flat: Vec<&str> = registry().node_info().fields.iter().filter_map(|f| f.lua).collect();
    for c in registry().components {
        assert!(c.info().fields.iter().all(|f| !f.doc.trim().is_empty()), "{}: a field has no doc", c.name);
        if c.lua == LuaScope::Object {
            flat.extend(c.info().fields.iter().filter_map(|f| f.lua));
        }
    }
    let n = flat.len();
    flat.sort_unstable();
    flat.dedup();
    assert_eq!(flat.len(), n, "a flat Luau name is claimed twice");
    for known in ["name", "visible", "position", "rotation", "scale", "mesh", "lod", "color", "metallic", "mass", "velocity", "shader"] {
        assert!(flat.contains(&known), "`{known}` is missing from the flat names");
    }
}

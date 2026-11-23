//! Commands on the scene environment: the reserved node address `@environment` (ADR 0005, "C-17 decisions").

use spark_core::doc::Environment;
use spark_core::{Command, CommandError, ComponentState, Fog, NodeDoc, NodeId, SceneDoc, Sky, World};
use spark_reflect::{ReflectError, Value};

fn env_id() -> NodeId {
    NodeId::address("@environment").unwrap()
}

fn set(path: &str, value: Value) -> Command {
    Command::SetProperty { node: env_id(), path: path.into(), value }
}

fn env(w: &World) -> Environment {
    Environment::of(&w.scene)
}

/// Applies, checks the environment changed, undoes through the inverse, checks it is identical.
fn roundtrip(w: &mut World, cmd: &Command) {
    let before = env(w);
    let applied = w.apply(cmd).unwrap_or_else(|e| panic!("{cmd:?}: {e}"));
    assert_ne!(before, env(w), "{cmd:?} changed nothing");
    assert_eq!(applied.changed, vec![env_id()]);
    w.apply(&applied.inverse).unwrap();
    assert_eq!(before, env(w), "the inverse of {cmd:?} did not restore the environment");
}

#[test]
fn sun_and_fog_set_and_undo() {
    let mut w = World::new();
    roundtrip(&mut w, &set("sun.intensity", Value::Float(2.5)));
    assert_eq!(w.apply(&set("sun.intensity", Value::Float(2.5))).unwrap().inverse, set("sun.intensity", Value::Float(1.0)));
    assert_eq!(w.scene.sun.intensity, 2.5);

    // fog is off by default: a leaf cannot be written until the fog exists; a whole fog turns it on, undo turns it off
    assert!(w.apply(&set("fog.color", Value::Color([1.0, 0.0, 0.0, 1.0]))).is_err());
    w.scene.sun.intensity = 1.0;
    let fog = Fog::default();
    let on = w.apply(&set("fog", spark_reflect::Reflect::to_value(&fog))).unwrap();
    assert_eq!(w.scene.fog, Some(fog));
    roundtrip(&mut w, &set("fog.color", Value::Color([1.0, 0.0, 0.0, 1.0])));
    roundtrip(&mut w, &set("fog.far", Value::Float(99.0)));
    w.apply(&on.inverse).unwrap();
    assert_eq!(w.scene.fog, None);

    roundtrip(&mut w, &set("ambient", Value::Color([0.5, 0.5, 0.5, 1.0])));
    roundtrip(&mut w, &set("background", Value::Color([0.0, 0.0, 0.2, 1.0])));
    roundtrip(&mut w, &set("sun.direction", Value::Vec3([0.0, -1.0, 0.0])));
}

#[test]
fn whole_sky_is_restored_exactly_and_the_cubemap_stays_read_only() {
    let mut w = World::new();
    let sky = Sky::gradient(spark_core::Color::WHITE, spark_core::Color::GRAY);
    w.scene.sky = Some(sky);
    roundtrip(&mut w, &set("sky.top", Value::Color([0.1, 0.2, 0.3, 1.0])));
    roundtrip(&mut w, &set("sky.blend", Value::Float(0.5)));
    roundtrip(&mut w, &set("sky", Value::None));
    let err = w.apply(&set("sky.cubemap", Value::Int(1))).unwrap_err();
    assert!(matches!(err, CommandError::Reflect(_)), "{err}");
}

#[test]
fn unknown_paths_suggest() {
    let mut w = World::new();
    let before = env(&w);
    let err = w.apply(&set("sun.intensty", Value::Float(2.0))).unwrap_err();
    let CommandError::Reflect(ReflectError::UnknownField { suggestions, .. }) = &err else { panic!("{err:?}") };
    assert!(suggestions.contains(&"intensity"), "{err}");
    let err = w.apply(&set("sunn.intensity", Value::Float(2.0))).unwrap_err();
    assert!(err.to_string().contains("sun"), "{err}");
    let err = w.apply(&set("sun.intensity", Value::Str("bright".into()))).unwrap_err();
    assert_eq!(err.code(), "wrong_type");
    assert_eq!(before, env(&w));
}

#[test]
fn other_commands_reject_the_environment() {
    let mut w = World::new();
    let e = env_id();
    let spawn_under = Command::Spawn { subtree: SceneDoc::new(NodeDoc::named("x")), parent: Some(e.clone()), index: None };
    let cmds = [
        spawn_under,
        Command::Delete { node: e.clone() },
        Command::Reparent { node: e.clone(), parent: None, index: None },
        Command::AddComponent { node: e.clone(), component: "body".into(), value: None },
        Command::RemoveComponent { node: e.clone(), component: "light".into() },
        Command::Restore { node: e.clone(), state: ComponentState::Transform(Default::default()) },
    ];
    for c in &cmds {
        let err = w.apply(c).unwrap_err();
        assert!(matches!(err, CommandError::Invalid(_)) && err.to_string().contains("@environment"), "{c:?}: {err}");
    }
    // under a real node it cannot be a parent either
    let id =
        w.apply(&Command::Spawn { subtree: SceneDoc::new(NodeDoc::named("n")), parent: None, index: None }).unwrap().changed[0].clone();
    assert!(w.apply(&Command::Reparent { node: id, parent: Some(e), index: None }).is_err());
    assert_eq!(w.scene.len(), 1);
    // and no node can be given such an id
    let mut doc = NodeDoc::named("bad");
    doc.id = NodeId::new("@environment").ok();
    assert!(doc.id.is_none());
}

#[test]
fn history_undoes_an_environment_edit() {
    let mut w = World::new();
    let mut h = spark_core::History::new(100);
    h.apply(&mut w, set("sun.intensity", Value::Float(3.0))).unwrap();
    assert_eq!(w.scene.sun.intensity, 3.0);
    h.undo(&mut w).unwrap();
    assert_eq!(w.scene.sun.intensity, 1.0);
    h.redo(&mut w).unwrap();
    assert_eq!(w.scene.sun.intensity, 3.0);
}

//! Commands, `World::apply`, inverses and `History` (ADR 0005, step C-12).

use std::fmt::Write as _;

use spark_core::{
    Body, Command, CommandError, ComponentState, History, Light, MeshData, MeshDoc, MeshSource, MeshState, NodeDoc, NodeId, ObjectId,
    SceneDoc, Transform, Vec3, World,
};
use spark_reflect::{Reflect, ReflectEnum, Value};

fn nid(s: &str) -> NodeId {
    NodeId::new(s).unwrap()
}

/// Deterministic dump of everything a command can change: node ids, hierarchy (children in order, roots sorted by
/// id: root order is the slot order of the scene and not part of the contract), exact component state.
fn dump(w: &World) -> String {
    fn node(w: &World, oid: ObjectId, depth: usize, out: &mut String) {
        let o = &w.scene[oid];
        let _ = writeln!(
            out,
            "{}{} {:?} vis={} {:?} {:?} body={:?} light={:?} mesh={:?}/{:?} lod={:?} bounds={:?}",
            "  ".repeat(depth),
            o.node_id().map_or("-".to_string(), |n| n.to_string()),
            o.name,
            o.visible,
            o.transform,
            o.material,
            o.body,
            o.light,
            o.mesh,
            o.mesh.map(|m| w.assets.mesh_source(m).clone()),
            o.lod,
            o.local_bounds(),
        );
        for c in o.child_ids() {
            node(w, *c, depth + 1, out);
        }
    }
    let mut roots: Vec<ObjectId> = w.scene.iter().filter(|(_, o)| o.parent().is_none()).map(|(id, _)| id).collect();
    roots.sort_by_key(|r| w.scene[*r].node_id().map(|n| n.to_string()));
    let mut out = String::new();
    for r in roots {
        node(w, r, 0, &mut out);
    }
    let _ = writeln!(out, "count={}", w.scene.len());
    out
}

/// root
///   a (box mesh, dynamic body)
///     c (empty)
///   b (light)
/// Built the way scripts build objects, then given ids.
fn fixture() -> World {
    let mut w = World::new();
    let mesh = w.assets.add_mesh_from(&MeshSource::Box { size: Vec3::ONE, tile: None, cell: None, surface: false }).unwrap();
    let root = w.spawn_empty();
    let a = w.spawn(mesh, Default::default());
    let b = w.spawn_empty();
    let c = w.spawn_empty();
    w.scene.set_parent(a, Some(root)).unwrap();
    w.scene.set_parent(b, Some(root)).unwrap();
    w.scene.set_parent(c, Some(a)).unwrap();
    w.scene[a].body = Some(Body::dynamic());
    w.scene[b].light = Some(Light::default());
    w.scene[a].transform.position = Vec3::new(1.0, 2.0, 3.0);
    for (oid, name) in [(root, "Root"), (a, "A"), (b, "B"), (c, "C")] {
        w.scene[oid].name = name.into();
        w.node_id(oid); // lazy ids: assign now so the dumps can compare them
    }
    w
}

/// Applies `cmd`, checks the state changed, applies the inverse and checks the state is identical, then applies
/// the command again (redo) and checks that gives the same state as the first time.
fn roundtrip(w: &mut World, cmd: &Command) {
    let before = dump(w);
    let a = w.apply(cmd).unwrap_or_else(|e| panic!("{cmd:?}: {e}"));
    let after = dump(w);
    assert_ne!(before, after, "{cmd:?} changed nothing");
    w.apply(&a.inverse).unwrap_or_else(|e| panic!("inverse of {cmd:?}: {e}"));
    assert_eq!(before, dump(w), "inverse of {cmd:?} did not restore the scene");
    let again = w.apply(a.resolved.as_ref().unwrap_or(cmd)).unwrap();
    assert_eq!(after, dump(w), "redo of {cmd:?} differs");
    w.apply(&again.inverse).unwrap(); // leave the scene as it was
    assert_eq!(before, dump(w), "second undo of {cmd:?}");
}

/// The fixture node ids by name.
fn ids(w: &World) -> (NodeId, NodeId, NodeId, NodeId) {
    let by = |n: &str| w.node_id_of(w.scene.find(n).unwrap()).unwrap().clone();
    (by("Root"), by("A"), by("B"), by("C"))
}

fn sample_subtree() -> SceneDoc {
    let mut child = NodeDoc::named("Child");
    child.id = Some(nid("kid"));
    child.light = Some(Light::default());
    let mut root = NodeDoc::named("New");
    root.id = Some(nid("fresh"));
    root.transform.position = Vec3::new(5.0, 0.0, 0.0);
    root.body = Some(Body::fixed());
    root.mesh = Some(MeshState { mesh: MeshDoc::from_source(MeshSource::Sphere { radius: 1.0, segments: 8 }), lod: Vec::new() });
    root.children = vec![child, NodeDoc::named("Second")];
    SceneDoc::new(root)
}

#[test]
fn spawn_roundtrip_and_ids() {
    let mut w = fixture();
    let (root, ..) = ids(&w);
    roundtrip(&mut w, &Command::Spawn { subtree: sample_subtree(), parent: None, index: None });
    roundtrip(&mut w, &Command::Spawn { subtree: sample_subtree(), parent: Some(root.clone()), index: Some(1) });
    // The subtree is where it was asked to be, with the ids it carried.
    w.apply(&Command::Spawn { subtree: sample_subtree(), parent: Some(root.clone()), index: Some(1) }).unwrap();
    let fresh = w.find_node(&nid("fresh")).unwrap();
    let kids: Vec<_> = w.scene[w.find_node(&root).unwrap()].child_ids().to_vec();
    assert_eq!(kids.len(), 3);
    assert_eq!(kids[1], fresh);
    assert_eq!(w.scene[fresh].child_ids().len(), 2);
    assert_eq!(w.find_node(&nid("kid")).map(|k| w.scene[k].name.clone()), Some("Child".to_string()));
    assert!(w.scene[fresh].local_bounds().min.x < 0.0, "mesh bounds are set");
}

#[test]
fn spawn_generates_ids_and_redo_keeps_them() {
    let mut w = fixture();
    let mut h = History::default();
    let doc = SceneDoc::new(NodeDoc { children: vec![NodeDoc::named("kid")], ..NodeDoc::named("gen") });
    let a = h.apply(&mut w, Command::Spawn { subtree: doc, parent: None, index: None }).unwrap();
    assert_eq!(a.changed.len(), 2);
    assert!(a.resolved.is_some(), "generated ids are written back");
    let after = dump(&w);
    h.undo(&mut w).unwrap();
    assert!(a.changed.iter().all(|n| w.find_node(n).is_none()));
    h.redo(&mut w).unwrap();
    assert_eq!(dump(&w), after, "redo recreates the same ids");
}

#[test]
fn delete_roundtrip_restores_subtree_ids_and_order() {
    let mut w = fixture();
    let (root, a, b, c) = ids(&w);
    // `a` has a body, a mesh and a child; deleting it must bring all back. Also from the middle of a child list.
    roundtrip(&mut w, &Command::Delete { node: a.clone() });
    roundtrip(&mut w, &Command::Delete { node: root.clone() });
    roundtrip(&mut w, &Command::Delete { node: b.clone() });
    roundtrip(&mut w, &Command::Delete { node: c.clone() });
    let before = dump(&w);
    let inv = w.apply(&Command::Delete { node: a.clone() }).unwrap().inverse;
    assert!(w.find_node(&a).is_none() && w.find_node(&c).is_none());
    w.apply(&inv).unwrap();
    assert!(w.find_node(&a).is_some() && w.find_node(&c).is_some(), "same ids come back");
    assert_eq!(before, dump(&w));
}

#[test]
fn delete_restores_runtime_only_meshes_within_the_session() {
    let mut w = World::new();
    let m = w.add_mesh(MeshData::cube(1.0)); // no recipe
    let o = w.spawn(m, Default::default());
    let id = w.node_id(o).unwrap();
    let before = dump(&w);
    let inv = w.apply(&Command::Delete { node: id }).unwrap().inverse;
    w.apply(&inv).unwrap();
    assert_eq!(before, dump(&w));
    // After the assets are gone the handle is stale and a recipe-less mesh cannot be rebuilt: an error, no change.
    let id = w.scene.iter().next().and_then(|(_, o)| o.node_id().cloned()).unwrap();
    let inv = w.apply(&Command::Delete { node: id }).unwrap().inverse;
    w.assets.clear();
    let err = w.apply(&inv).unwrap_err();
    assert!(matches!(err, CommandError::Invalid(_)), "{err}");
    assert!(w.scene.is_empty());
}

#[test]
fn set_property_roundtrips() {
    let mut w = fixture();
    let (_, a, b, _) = ids(&w);
    let s = |n: &NodeId, p: &str, v: Value| Command::SetProperty { node: n.clone(), path: p.into(), value: v };
    roundtrip(&mut w, &s(&a, "name", Value::Str("Other".into())));
    roundtrip(&mut w, &s(&a, "visible", Value::Bool(false)));
    roundtrip(&mut w, &s(&a, "transform.position", Value::Vec3([9.0, 8.0, 7.0])));
    roundtrip(&mut w, &s(&a, "transform.position.y", Value::Float(-3.5)));
    roundtrip(&mut w, &s(&a, "transform.rotation", Value::Vec3([10.0, 33.0, 77.0])));
    roundtrip(&mut w, &s(&a, "transform.scale", Value::Vec3([2.0, 2.0, 2.0])));
    roundtrip(&mut w, &s(&a, "material.color", Value::Color([0.25, 0.5, 0.75, 1.0])));
    roundtrip(&mut w, &s(&a, "body.mass", Value::Float(12.5)));
    roundtrip(&mut w, &s(&b, "light.intensity", Value::Float(3.0)));
    // A whole component, which is restored exactly (velocity is runtime state the value form does not carry).
    let oid = w.find_node(&a).unwrap();
    w.scene[oid].body.as_mut().unwrap().velocity = Vec3::new(1.0, 2.0, 3.0);
    let mut body = w.scene[w.find_node(&a).unwrap()].body.clone().unwrap().to_value();
    if let Value::Map(m) = &mut body {
        m.iter_mut().find(|(k, _)| k == "friction").unwrap().1 = Value::Float(0.9);
    }
    roundtrip(&mut w, &s(&a, "body", body));
}

#[test]
fn rotation_undo_is_bit_exact() {
    let mut w = fixture();
    let (_, a, ..) = ids(&w);
    let oid = w.find_node(&a).unwrap();
    w.scene[oid].transform.rotation = spark_core::Quat::from_xyzw(0.1, 0.7, 0.2, 0.3).normalize();
    let q = w.scene[oid].transform.rotation;
    let inv = w.apply(&set_n(&a, "transform.rotation", Value::Vec3([5.0, 6.0, 7.0]))).unwrap().inverse;
    w.apply(&inv).unwrap();
    assert_eq!(w.scene[oid].transform.rotation.to_array(), q.to_array());
}

fn set_n(n: &NodeId, p: &str, v: Value) -> Command {
    Command::SetProperty { node: n.clone(), path: p.into(), value: v }
}

#[test]
fn reparent_roundtrips() {
    let mut w = fixture();
    let (root, a, b, c) = ids(&w);
    let re = |n: &NodeId, p: Option<&NodeId>, i| Command::Reparent { node: n.clone(), parent: p.cloned(), index: i };
    roundtrip(&mut w, &re(&c, Some(&b), None));
    roundtrip(&mut w, &re(&c, None, None));
    roundtrip(&mut w, &re(&b, Some(&a), Some(0)));
    roundtrip(&mut w, &re(&b, Some(&root), Some(0))); // move within the same parent: [a, b] -> [b, a]
    let k = w.scene[w.find_node(&root).unwrap()].child_ids().to_vec();
    w.apply(&re(&b, Some(&root), Some(0))).unwrap();
    let k2 = w.scene[w.find_node(&root).unwrap()].child_ids().to_vec();
    assert_eq!(k2, vec![k[1], k[0]]);
}

#[test]
fn reparent_rejects_cycles() {
    let mut w = fixture();
    let (root, a, _, c) = ids(&w);
    let before = dump(&w);
    for (node, parent) in [(&root, &c), (&root, &a), (&a, &c), (&a, &a)] {
        let err = w.apply(&Command::Reparent { node: node.clone(), parent: Some(parent.clone()), index: None }).unwrap_err();
        assert!(matches!(err, CommandError::Cycle { .. }), "{err}");
        assert_eq!(err.code(), "cycle");
    }
    assert_eq!(before, dump(&w));
    let err = w.apply(&Command::Reparent { node: a, parent: Some(nid("nope")), index: None }).unwrap_err();
    assert_eq!(err, CommandError::NoSuchNode(nid("nope")));
}

#[test]
fn component_commands_roundtrip() {
    let mut w = fixture();
    let (_, a, b, c) = ids(&w);
    let add = |n: &NodeId, name: &str, v: Option<Value>| Command::AddComponent { node: n.clone(), component: name.into(), value: v };
    let rm = |n: &NodeId, name: &str| Command::RemoveComponent { node: n.clone(), component: name.into() };
    roundtrip(&mut w, &add(&c, "body", None)); // new
    roundtrip(&mut w, &add(&a, "body", Some(Body::fixed().to_value()))); // replaces: restored exactly
    roundtrip(&mut w, &add(&c, "light", None));
    roundtrip(&mut w, &add(&b, "light", Some(Light::spot(spark_core::Color::RED, 2.0, 5.0, 0.8, 0.2).to_value())));
    roundtrip(&mut w, &rm(&a, "body"));
    roundtrip(&mut w, &rm(&b, "light"));
    roundtrip(&mut w, &rm(&a, "mesh"));
    let sphere = MeshSource::Sphere { radius: 2.0, segments: 8 }.to_value();
    roundtrip(&mut w, &add(&c, "mesh", Some(sphere.clone())));
    roundtrip(&mut w, &add(&a, "mesh", Some(sphere))); // replaces the box
    roundtrip(
        &mut w,
        &Command::Restore {
            node: a.clone(),
            state: ComponentState::Transform(Transform { position: Vec3::new(4.0, 4.0, 4.0), ..Transform::default() }),
        },
    );
}

#[test]
fn component_errors() {
    let mut w = fixture();
    let (_, a, b, c) = ids(&w);
    let before = dump(&w);
    let e = |w: &mut World, c: Command| w.apply(&c).unwrap_err();
    let rm = |n: &NodeId, name: &str| Command::RemoveComponent { node: n.clone(), component: name.into() };
    assert!(matches!(e(&mut w, rm(&a, "transform")), CommandError::RequiredComponent { .. }));
    assert!(matches!(e(&mut w, rm(&c, "body")), CommandError::NoSuchComponent { .. }));
    match e(&mut w, rm(&a, "bodi")) {
        CommandError::UnknownComponent { suggestions, .. } => assert!(suggestions.contains(&"body")),
        other => panic!("{other}"),
    }
    assert!(matches!(e(&mut w, set_n(&c, "body.mass", Value::Float(1.0))), CommandError::NoSuchComponent { .. }));
    assert!(matches!(e(&mut w, set_n(&a, "body.mass", Value::Float(-1.0))), CommandError::Reflect(_)));
    assert!(matches!(e(&mut w, set_n(&a, "mesh.mesh", Value::Int(1))), CommandError::Reflect(_)));
    assert!(matches!(e(&mut w, set_n(&b, "nonsense", Value::Int(1))), CommandError::Reflect(_)));
    assert!(matches!(e(&mut w, set_n(&nid("zzz"), "name", Value::Str("x".into()))), CommandError::NoSuchNode(_)));
    let dup = Command::Spawn { subtree: SceneDoc::new(NodeDoc { id: Some(a.clone()), ..NodeDoc::default() }), parent: None, index: None };
    assert!(matches!(e(&mut w, dup), CommandError::DuplicateNode(_)));
    let twice = NodeDoc { id: Some(nid("t")), children: vec![NodeDoc { id: Some(nid("t")), ..NodeDoc::default() }], ..NodeDoc::default() };
    assert!(matches!(
        e(&mut w, Command::Spawn { subtree: SceneDoc::new(twice), parent: None, index: None }),
        CommandError::DuplicateNode(_)
    ));
    assert_eq!(before, dump(&w), "failed commands change nothing");
}

#[test]
fn batch_is_atomic() {
    let mut w = fixture();
    let (root, a, b, _) = ids(&w);
    let good = Command::Batch(vec![
        set_n(&a, "name", Value::Str("X".into())),
        Command::Reparent { node: b.clone(), parent: Some(a.clone()), index: None },
        Command::Spawn { subtree: sample_subtree(), parent: None, index: None },
    ]);
    roundtrip(&mut w, &good);

    let before = dump(&w);
    let bad = Command::Batch(vec![
        set_n(&a, "name", Value::Str("Changed".into())),
        Command::Delete { node: b.clone() },
        Command::Spawn { subtree: sample_subtree(), parent: None, index: None },
        Command::Reparent { node: a.clone(), parent: Some(a.clone()), index: None }, // fails: cycle
        set_n(&root, "name", Value::Str("never".into())),
    ]);
    assert!(matches!(w.apply(&bad), Err(CommandError::Cycle { .. })));
    assert_eq!(before, dump(&w), "the applied part was rolled back");
    assert!(w.find_node(&nid("fresh")).is_none() && w.find_node(&b).is_some());
}

#[test]
fn drag_merges_into_one_undo_step() {
    let mut w = fixture();
    let (_, a, b, _) = ids(&w);
    let start = dump(&w);
    let mut h = History::default();
    for i in 1..=20 {
        h.apply(&mut w, set_n(&a, "transform.position", Value::Vec3([i as f32, 0.0, 0.0]))).unwrap();
    }
    assert_eq!(h.len(), 1, "a drag is one step");
    assert_eq!(h.undo_label(), Some("Set transform.position"));
    let end = dump(&w);
    h.undo(&mut w).unwrap();
    assert_eq!(start, dump(&w), "undo goes back to before the drag");
    h.redo(&mut w).unwrap();
    assert_eq!(end, dump(&w), "redo goes to the last value");

    // A different path or node, another command in between, or a seal start a new step.
    h.apply(&mut w, set_n(&a, "transform.scale", Value::Vec3([2.0; 3]))).unwrap();
    h.apply(&mut w, set_n(&b, "transform.scale", Value::Vec3([2.0; 3]))).unwrap();
    assert_eq!(h.len(), 3);
    h.apply(&mut w, set_n(&b, "name", Value::Str("n1".into()))).unwrap();
    h.apply(&mut w, set_n(&b, "name", Value::Str("n2".into()))).unwrap();
    h.seal();
    h.apply(&mut w, set_n(&b, "name", Value::Str("n3".into()))).unwrap();
    assert_eq!(h.len(), 5);
    h.undo(&mut w).unwrap();
    assert_eq!(w.scene[w.find_node(&b).unwrap()].name, "n2");
    h.undo(&mut w).unwrap();
    assert_eq!(w.scene[w.find_node(&b).unwrap()].name, "B");
    // Recording after an undo clears redo.
    assert!(h.can_redo());
    h.apply(&mut w, set_n(&b, "name", Value::Str("n4".into()))).unwrap();
    assert!(!h.can_redo());
}

#[test]
fn history_limit_and_failed_commands() {
    let mut w = fixture();
    let (_, a, ..) = ids(&w);
    let mut h = History::new(3);
    for i in 0..10 {
        h.seal();
        h.apply(&mut w, set_n(&a, "name", Value::Str(format!("n{i}")))).unwrap();
    }
    assert_eq!(h.len(), 3);
    assert!(h.apply(&mut w, set_n(&a, "body.mass", Value::Float(0.0))).is_err());
    assert_eq!(h.len(), 3, "a failed command is not recorded");
    while h.can_undo() {
        h.undo(&mut w).unwrap();
    }
    assert_eq!(w.scene[w.find_node(&a).unwrap()].name, "n6");
    assert_eq!(h.undo(&mut w).unwrap(), Vec::<NodeId>::new(), "nothing to undo is not an error");
}

/// xorshift64*: a fixed seed gives the same sequence everywhere.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
    fn f(&mut self) -> f32 {
        (self.next() % 20_000) as f32 / 100.0 - 100.0
    }
    fn v3(&mut self) -> [f32; 3] {
        [self.f(), self.f(), self.f()]
    }
}

fn live_ids(w: &World) -> Vec<NodeId> {
    let mut v: Vec<NodeId> = w.scene.iter().filter_map(|(_, o)| o.node_id().cloned()).collect();
    v.sort();
    v
}

fn random_doc(r: &mut Rng, n: usize) -> NodeDoc {
    let mut d = NodeDoc::named(&format!("spawn{n}"));
    d.transform.position = Vec3::from(r.v3());
    if r.below(2) == 0 {
        d.body = Some(Body::dynamic());
    }
    if r.below(3) == 0 {
        d.light = Some(Light::default());
    }
    if r.below(2) == 0 {
        let source = MeshSource::Box { size: Vec3::new(1.0 + r.below(3) as f32, 1.0, 1.0), tile: None, cell: None, surface: false };
        d.mesh = Some(MeshState { mesh: MeshDoc::from_source(source), lod: Vec::new() });
    }
    if r.below(2) == 0 {
        d.children.push(NodeDoc::named(&format!("kid{n}")));
    }
    d
}

fn random_command(r: &mut Rng, w: &World, n: usize) -> Command {
    let ids = live_ids(w);
    let pick = |r: &mut Rng| ids[r.below(ids.len())].clone();
    match r.below(11) {
        0 | 1 => {
            let parent = (r.below(2) == 0).then(|| pick(r));
            Command::Spawn { subtree: SceneDoc::new(random_doc(r, n)), parent, index: (r.below(2) == 0).then(|| r.below(4)) }
        }
        2 if ids.len() > 4 => Command::Delete { node: pick(r) },
        3 => set_n(&pick(r), "transform.position", Value::Vec3(r.v3())),
        4 => set_n(&pick(r), "transform.rotation", Value::Vec3([r.f() % 80.0, r.f() % 170.0, r.f() % 170.0])),
        5 => set_n(&pick(r), "material.color", Value::Color([0.1, 0.2, 0.3, 1.0])),
        6 => {
            let node = pick(r);
            if r.below(2) == 0 {
                set_n(&node, "name", Value::Str(format!("name{n}")))
            } else {
                set_n(&node, "visible", Value::Bool(r.below(2) == 0))
            }
        }
        7 => Command::Reparent { node: pick(r), parent: (r.below(4) != 0).then(|| pick(r)), index: (r.below(2) == 0).then(|| r.below(4)) },
        8 => {
            let node = pick(r);
            let component = ["body", "light", "mesh"][r.below(3)].to_string();
            if r.below(2) == 0 {
                let value = (component == "mesh").then(|| MeshSource::Sphere { radius: 1.0 + r.below(3) as f32, segments: 8 }.to_value());
                Command::AddComponent { node, component, value }
            } else {
                Command::RemoveComponent { node, component }
            }
        }
        9 => Command::Batch(vec![
            set_n(&pick(r), "transform.position", Value::Vec3(r.v3())),
            set_n(&pick(r), "name", Value::Str(format!("b{n}"))),
            // Sometimes invalid (no body, or a mass of zero): the batch must roll back.
            set_n(&pick(r), "body.mass", Value::Float(if r.below(2) == 0 { 0.0 } else { 5.0 })),
        ]),
        _ => set_n(&pick(r), "body.mass", Value::Float(1.0 + r.below(9) as f64)),
    }
}

#[test]
fn random_sequence_undoes_and_redoes_exactly() {
    let mut w = fixture();
    let mut h = History::default();
    let initial = dump(&w);
    let (mut ok, mut failed) = (0, 0);
    let mut r = Rng(0x00C0_FFEE_1234_5678);
    let mut n = 0;
    while ok < 200 {
        n += 1;
        assert!(n < 5000, "the generator is stuck");
        let cmd = random_command(&mut r, &w, n);
        let before = dump(&w);
        h.seal(); // every command is its own step: 200 commands = 200 undos
        match h.apply(&mut w, cmd) {
            Ok(_) => ok += 1,
            Err(_) => {
                failed += 1;
                assert_eq!(before, dump(&w), "a failed command changed the scene");
            }
        }
    }
    assert!(failed > 5, "the sequence should also contain rejected commands ({failed})");
    assert_eq!(h.len(), 200);
    let last = dump(&w);
    assert_ne!(initial, last);

    let mut steps = 0;
    while h.can_undo() {
        h.undo(&mut w).unwrap();
        steps += 1;
    }
    assert_eq!(steps, 200);
    assert_eq!(initial, dump(&w), "undo of everything is the initial scene");

    while h.can_redo() {
        h.redo(&mut w).unwrap();
    }
    assert_eq!(last, dump(&w), "redo of everything is the final scene");
    while h.can_undo() {
        h.undo(&mut w).unwrap();
    }
    assert_eq!(initial, dump(&w));
}

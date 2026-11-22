//! Scripts attached to objects (ADR 0005, section 4): instances with props, callbacks in spawn order, and per-object
//! error isolation (an error or an infinite loop in one object does not stop the game or the other objects).

use std::path::PathBuf;
use std::time::Duration;

use spark_core::doc::{FileKind, FileNode, SceneFile, instantiate};
use spark_core::{Game, NodeId, ScriptComponent, World, run_frame};
use spark_reflect::Value as Prop;
use spark_script::ScriptGame;

static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

struct Run {
    game: ScriptGame,
    world: World,
    dir: PathBuf,
}

impl Run {
    fn new(main: &str, files: &[(&str, &str)]) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "spark-objscripts-{}-{}",
            std::process::id(),
            N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        for (name, src) in files {
            let p = dir.join(name);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, src).unwrap();
        }
        let path = dir.join("main.luau");
        std::fs::write(&path, main).unwrap();
        let mut game = ScriptGame::new(path).hot_reload(false).timeout(Some(Duration::from_millis(300)));
        let mut world = World::new();
        world.physics.set_backend(Box::new(spark_physics::RapierBackend::new()));
        game.start(&mut world);
        assert!(game.error().is_none(), "main.luau failed: {:?}", game.error());
        Self { game, world, dir }
    }

    fn frames(&mut self, n: usize) {
        for _ in 0..n {
            run_frame(&mut self.game, &mut self.world, 1.0 / 60.0);
        }
    }

    fn x(&self, name: &str) -> f32 {
        let id = self.world.scene.find(name).unwrap_or_else(|| panic!("no object named {name}"));
        self.world.scene[id].transform.position.x
    }

    fn failures(&self) -> Vec<String> {
        self.world.script_failures().iter().map(|f| f.message.clone()).collect()
    }
}

impl Drop for Run {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// A game with a main object that counts the global updates, plus the named plain objects.
fn main_with(extra: &str) -> String {
    format!(
        "local ticker\nfunction start()\n  ticker = spawn(Mesh.cube(), 'white', {{ name = 'ticker' }})\n  {extra}\nend\nfunction update(dt)\n  ticker.x = ticker.x + 1\nend\n"
    )
}

const COUNTER: &str = "local C = {}\nC.props = { step = 1 }\nfunction C:update(dt) self.x = self.x + self.step end\nreturn C\n";

#[test]
fn an_error_in_one_object_leaves_the_game_and_the_other_object_running() {
    let main = main_with(
        "local a = spawn(Mesh.cube(), 'red', { name = 'bad' })\n  local b = spawn(Mesh.cube(), 'blue', { name = 'good' })\n  a:attach_script('scripts/bad.luau')\n  b:attach_script('scripts/counter.luau')",
    );
    let bad = "local B = {}\nfunction B:start() self.n = 0 end\nfunction B:update(dt)\n  self.x = self.x + 1\n  self.n = self.n + 1\n  if self.n == 3 then error('boom') end\nend\nreturn B\n";
    let mut r = Run::new(&main, &[("scripts/bad.luau", bad), ("scripts/counter.luau", COUNTER)]);
    r.frames(10);
    assert!(r.game.error().is_none(), "the host must not stop: {:?}", r.game.error());
    assert_eq!(r.x("ticker"), 10.0, "the global update runs every frame");
    assert_eq!(r.x("good"), 10.0, "the healthy object keeps updating");
    assert_eq!(r.x("bad"), 3.0, "the failed object stops at its failure");
    let f = r.failures();
    assert_eq!(f.len(), 1, "reported once: {f:?}");
    assert!(f[0].contains("[SPK-1000]") && f[0].contains("scripts/bad.luau") && f[0].contains("boom"), "{}", f[0]);
    assert!(f[0].contains("\"bad\"") && f[0].contains("node "), "the object and its node id are named: {}", f[0]);
    let failure = &r.world.script_failures()[0];
    assert!(failure.node_id.is_some());
    assert_eq!(failure.object, "bad");
}

#[test]
fn a_global_error_still_stops_the_host() {
    let mut r = Run::new("function update(dt) error('global boom') end\n", &[]);
    r.frames(2);
    assert!(r.game.error().is_some_and(|e| e.contains("global boom")));
}

#[test]
fn props_from_the_file_override_the_defaults_and_unknown_names_are_errors() {
    let main = main_with("");
    let script = "local S = {}\nS.props = { speed = 2, label = 'door', offset = vec3(1, 2, 3) }\nfunction S:start()\n  self.x = self.speed * 10 + self.offset.y\nend\nreturn S\n";
    let mut r = Run::new(&main, &[("src/s.luau", script)]);
    let file_node = |id: &str, name: &str, props: Vec<(String, Prop)>| {
        let mut n = FileNode::new(NodeId::new(id).unwrap());
        n.name = name.into();
        n.set_component("script", ScriptComponent { path: "src/s.luau".into(), props }.to_value());
        n
    };
    let mut f = SceneFile::new(FileKind::Scene);
    f.nodes.push(file_node("a", "defaults", vec![]));
    f.nodes.push(file_node("b", "override", vec![("speed".into(), Prop::Int(5))]));
    f.nodes.push(file_node("c", "typo", vec![("spedd".into(), Prop::Int(5))]));
    f.nodes.push(file_node("d", "wrongtype", vec![("speed".into(), Prop::Str("fast".into()))]));
    f.nodes.push(file_node("e", "vec", vec![("offset".into(), Prop::Vec3([0.0, 4.0, 0.0]))]));
    let done = instantiate(&f, &mut r.world).unwrap();
    assert!(done.warnings.is_empty(), "scripts are attached, not ignored: {:?}", done.warnings);
    r.frames(2);
    assert!(r.game.error().is_none());
    assert_eq!(r.x("defaults"), 22.0);
    assert_eq!(r.x("override"), 52.0);
    assert_eq!(r.x("vec"), 24.0);
    let f = r.failures();
    assert_eq!(f.len(), 2, "{f:?}");
    let typo = f.iter().find(|m| m.contains("'spedd'")).expect("the typo is reported");
    assert!(typo.contains("did you mean 'speed'?") && typo.contains("[SPK-1008]") && typo.contains("src/s.luau"), "{typo}");
    let wrong = f.iter().find(|m| m.contains("\"wrongtype\"")).expect("the wrong type is reported");
    assert!(wrong.contains("must be a number") && wrong.contains("string"), "{wrong}");
    // the other objects of the file were not affected
    assert_eq!(r.x("ticker"), 2.0);
}

#[test]
fn a_prop_named_like_an_object_member_is_rejected() {
    let main = main_with("local a = spawn(Mesh.cube(), 'red', { name = 'a' })\n  a:attach_script('src/s.luau')");
    let script = "local S = {}\nS.props = { position = 1, speed = 2 }\nfunction S:update() end\nreturn S\n";
    let mut r = Run::new(&main, &[("src/s.luau", script)]);
    r.frames(2);
    assert!(r.game.error().is_none());
    let f = r.failures();
    assert_eq!(f.len(), 1, "{f:?}");
    assert!(f[0].contains("'position'") && f[0].contains("collides with the Object member obj.position"), "{}", f[0]);
    assert!(f[0].contains("[SPK-1008]"), "{}", f[0]);
}

#[test]
fn an_infinite_loop_in_one_object_is_cut_without_stopping_the_others() {
    let main = main_with(
        "local a = spawn(Mesh.cube(), 'red', { name = 'loop' })\n  local b = spawn(Mesh.cube(), 'blue', { name = 'good' })\n  a:attach_script('src/loop.luau')\n  b:attach_script('src/counter.luau')",
    );
    let looping = "local L = {}\nfunction L:update(dt) while true do end end\nreturn L\n";
    let mut r = Run::new(&main, &[("src/loop.luau", looping), ("src/counter.luau", COUNTER)]);
    let t = std::time::Instant::now();
    r.frames(5);
    assert!(t.elapsed() < Duration::from_secs(5), "the watchdog cuts the loop once, then the object is skipped");
    assert!(r.game.error().is_none(), "{:?}", r.game.error());
    assert_eq!(r.x("good"), 5.0);
    assert_eq!(r.x("ticker"), 5.0);
    let f = r.failures();
    assert_eq!(f.len(), 1, "{f:?}");
    assert!(f[0].contains("[SPK-1005]") && f[0].contains("infinite loop"), "{}", f[0]);
}

#[test]
fn a_misspelled_callback_is_an_error_with_a_suggestion() {
    let main = main_with("local a = spawn(Mesh.cube(), 'red', { name = 'a' })\n  a:attach_script('src/s.luau')");
    let script = "local S = {}\nfunction S:Update(dt) end\nfunction S:on_colision(o) end\nfunction S:helper() end\nreturn S\n";
    let mut r = Run::new(&main, &[("src/s.luau", script)]);
    r.frames(1);
    let f = r.failures();
    assert_eq!(f.len(), 1, "{f:?}");
    assert!(f[0].contains("'Update' is not a Spark callback (did you mean 'update'?)"), "{}", f[0]);
    assert!(f[0].contains("'on_colision'") && f[0].contains("did you mean 'on_collision'?"), "{}", f[0]);
    assert!(!f[0].contains("helper"), "{}", f[0]);
    assert!(f[0].contains("[SPK-1002]"), "{}", f[0]);
}

#[test]
fn on_destroy_runs_for_objects_removed_by_the_scene() {
    let main = main_with(
        "spawn(Mesh.cube(), 'white', { name = 'flag' })\n  local b = spawn(Mesh.cube(), 'red', { name = 'b' })\n  b:attach_script('src/d.luau')",
    );
    let script = "local D = {}\nfunction D:on_destroy()\n  local flag = find('flag')\n  flag.x = flag.x + 1\n  if self.object:exists() then flag.y = flag.y + 1 end\nend\nreturn D\n";
    let mut r = Run::new(&main, &[("src/d.luau", script)]);
    r.frames(2);
    assert_eq!(r.x("flag"), 0.0);
    let b = r.world.scene.find("b").unwrap();
    r.world.scene.destroy(b); // not through obj:destroy(): the host notices at its next sync point
    r.frames(2);
    assert_eq!(r.x("flag"), 1.0, "on_destroy ran once");
    let flag = r.world.scene.find("flag").unwrap();
    assert_eq!(r.world.scene[flag].transform.position.y, 0.0, "the object is already gone in that case");
    assert!(r.failures().is_empty(), "{:?}", r.failures());
}

#[test]
fn obj_destroy_calls_on_destroy_while_the_object_exists() {
    let main = "local a\nfunction start()\n  spawn(Mesh.cube(), 'white', { name = 'flag' })\n  a = spawn(Mesh.cube(), 'red', { name = 'a' })\n  a:attach_script('src/d.luau')\n  local kid = spawn(Mesh.cube(), 'red', { name = 'kid', parent = a })\n  kid:attach_script('src/d.luau')\nend\nlocal frame = 0\nfunction update(dt)\n  frame = frame + 1\n  if frame == 3 then a:destroy() end\nend\n";
    let script = "local D = {}\nfunction D:on_destroy()\n  local flag = find('flag')\n  flag.x = flag.x + 1\n  if self.object:exists() then flag.y = flag.y + 1 end\nend\nreturn D\n";
    let mut r = Run::new(main, &[("src/d.luau", script)]);
    r.frames(6);
    assert!(r.game.error().is_none(), "{:?}", r.game.error());
    assert_eq!(r.x("flag"), 2.0, "the object and its child both got on_destroy, once");
    let flag = r.world.scene.find("flag").unwrap();
    assert_eq!(r.world.scene[flag].transform.position.y, 2.0, "and the objects still existed");
    assert!(r.failures().is_empty(), "{:?}", r.failures());
}

#[test]
fn callbacks_run_in_attach_order_not_slot_order() {
    let main = main_with("spawn(Mesh.cube(), 'white', { name = 'order' })");
    let script = "local O = {}\nO.props = { tag = 0 }\nfunction O:update(dt)\n  local o = find('order')\n  o.x = o.x * 10 + self.tag\nend\nreturn O\n";
    let mut r = Run::new(&main, &[("src/o.luau", script)]);
    let attach = |r: &mut Run, tag: i64| {
        let id = r.world.spawn_empty();
        r.world.scene.set_script(id, Some(ScriptComponent { path: "src/o.luau".into(), props: vec![("tag".into(), Prop::Int(tag))] }));
        id
    };
    let a = attach(&mut r, 1);
    attach(&mut r, 2);
    r.world.scene.destroy(a);
    let c = attach(&mut r, 3); // reuses the slot of `a`
    assert_eq!(c.index(), a.index(), "the slot was reused");
    r.frames(1);
    assert_eq!(r.x("order"), 23.0, "2 was attached before 3, whatever the slots are");
    // a second frame repeats the same order
    r.frames(1);
    assert_eq!(r.x("order"), 2323.0);
}

#[test]
fn self_forwards_object_members_and_keeps_its_own_state() {
    let main = main_with(
        "local a = spawn(Mesh.cube(), 'red', { name = 'a', position = vec3(1, 0, 0) })\n  a:attach_script('src/s.luau', { turns = 4 })\n  assert(a.script == 'src/s.luau')",
    );
    let script = "local S = {}\nS.props = { turns = 1 }\nfunction S:start()\n  self.start_x = self.x\n  self.seen = self.object.name\nend\nfunction S:update(dt)\n  self.x = self.start_x + self.turns\n  self:rotate_y(0)\n  self.position = vec3(self.x, 5, 0)\n  self.visible = self.seen == 'a'\nend\nreturn S\n";
    let mut r = Run::new(&main, &[("src/s.luau", script)]);
    r.frames(2);
    assert!(r.game.error().is_none(), "{:?}", r.game.error());
    assert!(r.failures().is_empty(), "{:?}", r.failures());
    let a = r.world.scene.find("a").unwrap();
    let o = &r.world.scene[a];
    assert_eq!((o.transform.position.x, o.transform.position.y), (5.0, 5.0));
    assert!(o.visible);
    assert_eq!(o.script().unwrap().path, "src/s.luau");
    assert_eq!(o.script().unwrap().props, vec![("turns".to_string(), Prop::Int(4))]);
}

#[test]
fn on_collision_is_called_on_the_object_that_was_hit() {
    let main = "function start()\n  spawn(Mesh.cube(), 'gray', { name = 'ground', position = vec3(0, -0.5, 0), scale = vec3(10, 1, 10), body = 'static' })\n  spawn(Mesh.cube(), 'white', { name = 'flag' })\n  local ball = spawn(Mesh.cube(), 'red', { name = 'ball', position = vec3(0, 1.5, 0), body = 'dynamic' })\n  ball:attach_script('src/hit.luau')\nend\n";
    let script = "local H = {}\nfunction H:on_collision(other, info)\n  local flag = find('flag')\n  flag.x = flag.x + 1\n  flag.y = (other.name == 'ground') and 1 or 0\nend\nreturn H\n";
    let mut r = Run::new(main, &[("src/hit.luau", script)]);
    r.frames(120);
    assert!(r.game.error().is_none(), "{:?}", r.game.error());
    assert!(r.failures().is_empty(), "{:?}", r.failures());
    assert!(r.x("flag") >= 1.0, "the ball touched the ground");
    let flag = r.world.scene.find("flag").unwrap();
    assert_eq!(r.world.scene[flag].transform.position.y, 1.0, "`other` is the ground");
}

#[test]
fn a_script_that_does_not_return_a_table_fails_that_object_only() {
    let main = main_with(
        "local a = spawn(Mesh.cube(), 'red', { name = 'a' })\n  a:attach_script('src/none.luau')\n  local m = spawn(Mesh.cube(), 'red', { name = 'missing' })\n  m:attach_script('src/nothing_here.luau')",
    );
    let mut r = Run::new(&main, &[("src/none.luau", "local x = 1\n")]);
    r.frames(2);
    assert!(r.game.error().is_none());
    let f = r.failures();
    assert_eq!(f.len(), 2, "{f:?}");
    assert!(f.iter().any(|m| m.contains("must return a table")), "{f:?}");
    assert!(f.iter().any(|m| m.contains("no such module")), "{f:?}");
    assert_eq!(r.x("ticker"), 2.0);
}

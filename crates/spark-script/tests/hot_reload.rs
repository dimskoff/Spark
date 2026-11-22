//! Hot reload of one module (ADR 0006): the world, the objects and the loaded assets stay.

mod common;

use std::path::PathBuf;
use std::time::{Duration, Instant};

use spark_core::{Game, World, run_frame};
use spark_script::ScriptGame;

static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

struct Run {
    game: ScriptGame,
    world: World,
    dir: PathBuf,
}

impl Run {
    fn new(files: &[(&str, &str)]) -> Self {
        let dir =
            std::env::temp_dir().join(format!("spark-hot-{}-{}", std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
        std::fs::create_dir_all(&dir).unwrap();
        for (name, src) in files {
            let p = dir.join(name);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, src).unwrap();
        }
        let mut game = ScriptGame::new(dir.join("main.luau")).timeout(Some(Duration::from_secs(5)));
        let mut world = World::new();
        game.start(&mut world);
        assert!(game.error().is_none(), "{:?}", game.error());
        Self { game, world, dir }
    }

    fn frames(&mut self, n: usize) {
        for _ in 0..n {
            run_frame(&mut self.game, &mut self.world, 1.0 / 60.0);
        }
    }

    /// Writes a file and runs frames until the host noticed (a module reload or a restart). Returns the wait.
    fn save(&mut self, name: &str, src: &str) -> Duration {
        std::thread::sleep(Duration::from_millis(20));
        std::fs::write(self.dir.join(name), src).unwrap();
        let before = self.game.module_reloads().0;
        let t = Instant::now();
        let epoch = self.world.resets();
        while self.game.module_reloads().0 == before && self.world.resets() == epoch {
            assert!(t.elapsed() < Duration::from_secs(5), "the change was not noticed");
            self.frames(1);
            std::thread::sleep(Duration::from_millis(2));
        }
        t.elapsed()
    }

    fn x(&self, name: &str) -> f32 {
        let id = self.world.scene.find(name).unwrap_or_else(|| panic!("no object {name}"));
        self.world.scene[id].transform.position.x
    }
}

impl Drop for Run {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

const MAIN: &str = "local M = require('./src/mod')\nlocal o\nfunction start() o = spawn(Mesh.cube(), 'white', { name = 'o' }) end\nfunction update(dt) o.x = M.step() end\n";

#[test]
fn a_module_change_keeps_the_world_and_swaps_the_code() {
    let mut r = Run::new(&[("main.luau", MAIN), ("src/mod.luau", "local M = {}\nfunction M.step() return 1 end\nreturn M\n")]);
    r.frames(3);
    assert_eq!(r.x("o"), 1.0);
    let epoch = r.world.resets();
    r.save("src/mod.luau", "local M = {}\nfunction M.step() return 10 end\nreturn M\n");
    assert_eq!(r.world.resets(), epoch, "no restart");
    assert_eq!(r.world.scene.len(), 1, "the object is still there");
    r.frames(2);
    assert_eq!(r.x("o"), 10.0, "the new code runs: main.luau holds the same table");
    assert!(r.game.error().is_none());
}

#[test]
fn on_reload_gets_the_old_state_and_classes_keep_working_across_reloads() {
    let v = |n: u32| {
        format!(
            "local C = {{}}\nC.__index = C\nC.count = 0\nC.version = {n}\nfunction C.on_reload(old) C.count = (old.count or 0) + 100 end\n\
             function C.new() C.count = C.count + 1 return setmetatable({{}}, C) end\nfunction C:ver() return C.version * 1000 + C.count end\nreturn C\n"
        )
    };
    let main = "local C = require('./src/c')\nlocal a\nfunction start() a = C.new() end\nfunction update(dt) spawn(Mesh.cube(), 'white', { name = 'v' .. a:ver() }) end\n";
    let mut r = Run::new(&[("main.luau", main), ("src/c.luau", &v(1))]);
    r.frames(1);
    assert!(r.world.scene.find("v1001").is_some());
    r.save("src/c.luau", &v(2));
    r.frames(1);
    // old instance (metatable of the first table), count carried over by on_reload(old)
    assert!(r.world.scene.find("v2101").is_some(), "count 1 + 100, version 2");
    r.save("src/c.luau", &v(3));
    r.frames(1);
    assert!(r.world.scene.find("v3201").is_some(), "after a second reload too");
    assert!(r.game.error().is_none());
}

#[test]
fn a_broken_module_keeps_the_old_version_and_a_fix_reloads() {
    let mut r = Run::new(&[("main.luau", MAIN), ("src/mod.luau", "local M = {}\nfunction M.step() return 1 end\nreturn M\n")]);
    r.save("src/mod.luau", "local M = {\nreturn M\n");
    assert!(Game::error(&r.game).is_some(), "reported");
    r.frames(2);
    assert_eq!(r.x("o"), 1.0, "the old code keeps running");
    r.save("src/mod.luau", "local M = {}\nfunction M.step() return 5 end\nreturn M\n");
    assert!(Game::error(&r.game).is_none());
    r.frames(1);
    assert_eq!(r.x("o"), 5.0);
}

#[test]
fn a_module_returning_a_function_restarts_and_main_restarts() {
    let mut r = Run::new(&[("main.luau", MAIN.replace("M.step()", "M()").as_str()), ("src/mod.luau", "return function() return 1 end\n")]);
    r.frames(2);
    let epoch = r.world.resets();
    r.save("src/mod.luau", "return function() return 2 end\n");
    assert!(r.world.resets() > epoch, "restart");
    r.frames(1);
    assert_eq!(r.x("o"), 2.0);
    let epoch = r.world.resets();
    r.save("main.luau", &MAIN.replace("M.step()", "M()"));
    assert!(r.world.resets() > epoch, "main.luau restarts the game");
}

#[test]
fn object_scripts_keep_their_state_and_get_the_new_code() {
    let door = |step: u32| {
        format!(
            "local D = {{}}\nD.props = {{ step = 1 }}\nfunction D:start() self.n = 7 end\nfunction D:update(dt) self.x = {step} self.y = self.n end\nreturn D\n"
        )
    };
    let main = "function start() local d = spawn(Mesh.cube(), 'white', { name = 'd' }) d:attach_script('scripts/door.luau') end\n";
    let mut r = Run::new(&[("main.luau", main), ("scripts/door.luau", &door(1))]);
    r.frames(3);
    assert_eq!(r.x("d"), 1.0);
    r.save("scripts/door.luau", &door(5));
    r.frames(1);
    assert_eq!(r.x("d"), 5.0, "new update, same instance");
    let id = r.world.scene.find("d").unwrap();
    assert_eq!(r.world.scene[id].transform.position.y, 7.0, "self.n from start() survived");
}

#[test]
fn a_reloaded_module_sees_globals_the_game_changed_later() {
    let main = "score = 1\nlocal M = require('./src/m')\nfunction update(dt) score = score + 1 end\nfunction get() return M.read() end\n";
    let m = |k: u32| format!("local M = {{}}\nfunction M.read() return score * {k} end\nreturn M\n");
    let main = format!(
        "{main}local o\nfunction start() o = spawn(Mesh.cube(), 'white', {{ name = 'o' }}) end\nfunction late_update(dt) o.x = get() end\n"
    );
    let mut r = Run::new(&[("main.luau", &main), ("src/m.luau", &m(1))]);
    r.frames(3);
    assert!(r.x("o") >= 4.0);
    r.save("src/m.luau", &m(10));
    r.frames(1);
    assert!(r.x("o") >= 50.0 && r.x("o") % 10.0 == 0.0, "score is read live: {}", r.x("o"));
}

/// The measurement of the roadmap: saving a module in a game with 50 models.
#[test]
fn saving_a_module_with_50_models_loaded_takes_under_100_ms() {
    let main = "local M = require('./src/mod')\nfunction start()\n  for i = 1, 50 do\n    local m = Model.load('assets/rig' .. i .. '.glb')\n    spawn(m, 'white', { position = vec3(i, 0, 0) })\n  end\nend\nfunction update(dt) M.tick() end\n";
    let dir = std::env::temp_dir().join(format!("spark-hot-50-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("src")).unwrap();
    for i in 1..=50 {
        common::write_test_rig(&dir.join(format!("assets/rig{i}.glb")));
    }
    std::fs::write(dir.join("main.luau"), main).unwrap();
    std::fs::write(dir.join("src/mod.luau"), "local M = {}\nfunction M.tick() end\nreturn M\n").unwrap();
    let mut game = ScriptGame::new(dir.join("main.luau")).timeout(None);
    let mut world = World::new();
    let t = Instant::now();
    game.start(&mut world);
    let full_start = t.elapsed();
    assert!(game.error().is_none(), "{:?}", game.error());
    assert!(world.scene.len() >= 50, "50 models spawned: {}", world.scene.len());
    let objects = world.scene.len();
    for _ in 0..3 {
        run_frame(&mut game, &mut world, 1.0 / 60.0);
    }
    let mut times = Vec::new();
    for n in 0..5 {
        std::thread::sleep(Duration::from_millis(20));
        std::fs::write(dir.join("src/mod.luau"), format!("local M = {{}}\nfunction M.tick() end\nM.n = {n}\nreturn M\n")).unwrap();
        let before = game.module_reloads().0;
        while game.module_reloads().0 == before {
            run_frame(&mut game, &mut world, 1.0 / 60.0);
            std::thread::sleep(Duration::from_millis(1));
        }
        times.push(game.module_reloads().1.unwrap());
    }
    eprintln!("full start with 50 models: {full_start:?}; module reloads: {times:?}");
    assert_eq!(world.scene.len(), objects, "the scene is untouched");
    for t in &times {
        assert!(*t < Duration::from_millis(100), "reload took {t:?}");
    }
    drop(game);
    let _ = std::fs::remove_dir_all(&dir);
}

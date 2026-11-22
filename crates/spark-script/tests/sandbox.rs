//! Luau sandbox: libraries and Spark API tables are read-only; user globals, modules and tasks share one writable environment.

use spark_core::{Game, World, run_frame};
use spark_script::ScriptGame;

static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Runs `main` (plus extra `files`) for `frames` frames, returns the script error (if any).
fn run_files(main: &str, files: &[(&str, &str)], frames: usize) -> Option<String> {
    let dir =
        std::env::temp_dir().join(format!("spark-sandbox-{}-{}", std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
    std::fs::create_dir_all(&dir).unwrap();
    for (name, src) in files {
        let p = dir.join(name);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, src).unwrap();
    }
    let path = dir.join("main.luau");
    std::fs::write(&path, main).unwrap();
    let mut game = ScriptGame::new(path).hot_reload(false).timeout(Some(std::time::Duration::from_millis(300)));
    let mut world = World::new();
    world.physics.set_backend(Box::new(spark_physics::RapierBackend::new()));
    game.start(&mut world);
    for _ in 0..frames {
        if game.error().is_some() {
            break;
        }
        run_frame(&mut game, &mut world, 1.0 / 60.0);
    }
    let e = game.error().map(str::to_string);
    drop(game);
    let _ = std::fs::remove_dir_all(&dir);
    e
}

fn run(source: &str) -> Option<String> {
    run_files(source, &[], 6)
}

fn must_fail(source: &str) -> String {
    run(source).unwrap_or_else(|| panic!("expected an error for:\n{source}"))
}

#[test]
fn luau_libraries_are_read_only() {
    for src in [
        "math.sin = nil",
        "math.sin = function() return 0 end",
        "string.rep = nil",
        "table.insert = 1",
        "setmetatable(string, {})",
        "math.newthing = 1",
        "rawset(math, 'sin', 1)",
    ] {
        let e = must_fail(src);
        assert!(e.contains("cannot modify a built-in library"), "{src}: {e}");
        assert!(e.contains("main.luau:1"), "{src}: {e}");
        assert!(!e.contains("attempt to modify a readonly table"), "{src}: {e}");
    }
    // Inside a callback too, and the error stays classified.
    let e = must_fail("function update(dt) math.sin = nil end");
    assert!(e.contains("cannot modify a built-in library"), "{e}");
    assert_eq!(spark_script::classify(&e), "SPK-1007");
}

#[test]
fn spark_api_tables_are_read_only() {
    for src in ["Mesh.cube = nil", "Mesh.mine = 1", "input.down = nil", "ui.label = nil", "draw.rect = nil", "Color.rgb = nil"] {
        let e = must_fail(src);
        assert!(e.contains("cannot modify a built-in library"), "{src}: {e}");
    }
}

#[test]
fn builtin_metatables_are_read_only() {
    let e = must_fail("getmetatable('').__index = {}");
    assert!(e.contains("cannot modify a built-in library"), "{e}");
}

#[test]
fn base_globals_cannot_be_reached_and_changed() {
    // The read-only table behind the script's own globals: reachable for reading, never for writing.
    let e = must_fail("getmetatable(_G).__index.spawn = nil");
    assert!(e.contains("cannot modify a built-in library"), "{e}");
    let e = must_fail("getmetatable(_G).__index = {}");
    assert!(e.contains("cannot modify a built-in library"), "{e}");
    let e = must_fail("local base = getmetatable(_G).__index\nrawset(base, 'x', 1)");
    assert!(e.contains("cannot modify a built-in library"), "{e}");
    // An own global may shadow a built-in name (only for the script), the engine's tables stay intact.
    let e = run("function spawn_all() end\nlocal real = spawn\nspawn = 5\nassert(getmetatable(_G).__index.spawn == real)\n");
    assert_eq!(e, None);
}
#[test]
fn user_globals_and_callbacks_work() {
    let e = run_files(
        "score = 0\nticks = 0\nfunction update(dt) score += 1 end\nfunction fixed_update(dt) ticks += 1 end\n\
         function late_update(dt) assert(score >= 1) end\nfunction render() assert(score >= 0) end\n\
         function helper() return 7 end\nassert(helper() == 7)\n\
         _G.viaG = 5\nassert(viaG == 5)\nassert(rawget(_G, 'viaG') == 5)\nassert(rawget(_G, 'nothing') == nil)\n\
         function start() started = true end\n\
         task(function() wait(0.02) assert(started == true) assert(score >= 1) done_task = true end)\n\
         function check() assert(done_task == true) end\n\
         task(function() wait(0.2) check() end)\n",
        &[],
        30,
    );
    assert_eq!(e, None);
}

#[test]
fn callbacks_really_run() {
    // update() must be found by the engine in the script's environment: it crashes the game on purpose.
    let e = must_fail("function update(dt) error('update ran') end");
    assert!(e.contains("update ran"), "{e}");
    let e = must_fail("function start() error('start ran') end");
    assert!(e.contains("start ran"), "{e}");
    let e = must_fail("function render() error('render ran') end");
    assert!(e.contains("render ran"), "{e}");
    let e = must_fail("function fixed_update(dt) error('fixed ran') end");
    assert!(e.contains("fixed ran"), "{e}");
    let e = must_fail("function late_update(dt) error('late ran') end");
    assert!(e.contains("late ran"), "{e}");
}

#[test]
fn modules_share_globals_and_define_functions() {
    let e = run_files(
        "shared = 1\nlocal m = require('./mod')\nassert(m.get() == 2, 'module saw ' .. tostring(m.get()))\n\
         assert(from_module() == 'hi')\nassert(from_module_value == 42)\n\
         function update(dt) assert(m.get() == 2) end\n",
        &[(
            "mod.luau",
            "shared += 1\nfunction from_module() return 'hi' end\nfrom_module_value = 42\nlocal M = {}\nfunction M.get() return shared end\nreturn M\n",
        )],
        6,
    );
    assert_eq!(e, None);
    // A module cannot patch a library either.
    let e = run_files("require('./bad')", &[("bad.luau", "math.floor = nil\n")], 2).expect("must fail");
    assert!(e.contains("cannot modify a built-in library") && e.contains("bad.luau:1"), "{e}");
}

#[test]
fn tasks_and_coroutines_see_globals() {
    let e = run("counter = 0\nlocal co = coroutine.create(function() counter += 1 coroutine.yield() counter += 10 end)\n\
         coroutine.resume(co)\nassert(counter == 1)\ncoroutine.resume(co)\nassert(counter == 11)\n\
         function start() wait(0.01) counter += 100 assert(helper_defined_later() == 1) end\n\
         function helper_defined_later() return 1 end\n\
         function update() assert(counter >= 11) end\n");
    assert_eq!(e, None);
}

#[test]
fn strict_globals_still_work_with_suggestions() {
    let e = must_fail("function update(dt) local x = mth.sin(1) end\n");
    assert!(e.contains("unknown global 'mth' (did you mean 'math'?)"), "{e}");
    // Suggestions include the script's own globals.
    let e = must_fail("player_speed = 1\nlocal x = playr_speed\n");
    assert!(e.contains("unknown global 'playr_speed' (did you mean 'player_speed'?)"), "{e}");
    let e = must_fail("local x = totally_undefined_thing\n");
    assert!(e.contains("unknown global 'totally_undefined_thing'") && !e.contains("did you mean"), "{e}");
}

#[test]
fn user_may_shadow_in_own_game_but_not_draw_callback() {
    let e = run("function spawn_enemy() end\nlocal function sin() end\nfunction update(dt) end\n");
    assert_eq!(e, None);
    let e = must_fail("function draw() end");
    assert!(e.contains("'draw' is Spark's 2D drawing library"), "{e}");
}

#[test]
fn gc_and_memory_helpers_keep_working() {
    let e = run("local before = collectgarbage('count')\nassert(before > 0)\nlocal t = {}\nfor i = 1, 1000 do t[i] = {i} end\n\
         assert(#t == 1000)\nassert(string.rep('a', 3) == 'aaa')\nassert(math.sin(0) == 0)\nassert(table.concat({'a','b'}, ',') == 'a,b')\n");
    assert_eq!(e, None);
}

#[test]
fn computed_properties_are_not_frozen_at_load() {
    // Under the sandbox Luau resolves `a.b` imports once at load; properties computed on every read must not be.
    let e = run("function start()\n  screen.height = 600\n  assert(screen.height == 600, 'screen.height ' .. screen.height)\n  \
         sound.volume = 0.25\n  assert(sound.volume == 0.25, 'sound.volume')\n  graphics.msaa = 1\n  assert(graphics.msaa == 1, 'msaa')\n  \
         camera.fov = 50\n  assert(camera.fov == 50, 'fov')\nend\nfunction update(dt) assert(time.dt > 0, 'time.dt') end\n");
    assert_eq!(e, None);
}

// Luau bakes global reads into the bytecode when a chunk loads; a script's own globals must still always be current.

#[test]
fn modules_see_globals_changed_later_by_main() {
    let e = run_files(
        "flag = false\nlocal m = require('./reader')\nfunction update(dt) flag = true\nassert(m.get() == true, 'module read stale flag') end\n",
        &[("reader.luau", "local M = {}\nfunction M.get() return flag end\nreturn M\n")],
        4,
    );
    assert_eq!(e, None);
}

#[test]
fn main_sees_globals_set_later_by_module() {
    let e = run_files(
        "counter = 0\nlocal m = require('./writer')\nfunction update(dt) m.bump() assert(counter == m.n(), 'main read stale counter ' .. counter) end\n",
        &[("writer.luau", "local M = {}\nlocal n = 0\nfunction M.bump() n += 1 counter = n end\nfunction M.n() return n end\nreturn M\n")],
        5,
    );
    assert_eq!(e, None);
}

#[test]
fn globals_created_after_a_module_loaded() {
    let e = run_files(
        "local m = require('./late')\nfunction update(dt) late_global = (rawget(_G, 'late_global') or 0) + 1\nassert(m.get() == late_global, 'stale') end\n",
        &[(
            "late.luau",
            "local M = {}\nfunction M.get() return rawget(_G, 'late_global') end\nfunction M.get2() return late_global end\nreturn M\n",
        )],
        5,
    );
    assert_eq!(e, None);
    // Reading the global directly (strict mode errors until it exists, then must see updates).
    let e = run_files(
        "local m = require('./late')\nfunction update(dt) late_global = (rawget(_G, 'late_global') or 0) + 1\nassert(m.get2() == late_global, 'stale') end\n",
        &[("late.luau", "local M = {}\nfunction M.get2() return late_global end\nreturn M\n")],
        5,
    );
    assert_eq!(e, None);
}

#[test]
fn builtin_names_still_resolve_in_every_chunk() {
    let e = run_files(
        "local m = require('./m')\nassert(m.f() == 1)\nassert(Mesh.cube ~= nil and math.floor(2.5) == 2)\n",
        &[("m.luau", "local M = {}\nfunction M.f() return math.floor(1.5) + (Mesh.cube and 0 or 5) end\nreturn M\n")],
        2,
    );
    assert_eq!(e, None);
}

#[test]
fn restart_keeps_the_environment_working() {
    // restart() reloads the game (same path as hot reload): a fresh Lua state and sandbox each time.
    let e = run("if rawget(_G, 'runs') == nil then runs = 1 else error('state leaked across the reload') end\nflag = false\n\
         function update(dt) flag = true assert(flag) if not rawget(_G, 'asked') then asked = true restart() end end\n");
    assert_eq!(e, None);
}

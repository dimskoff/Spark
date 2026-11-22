//! "Zero silent errors": typos in callbacks, event names, globals and fields fail loudly with a suggestion.

use spark_core::{Game, World, run_frame};
use spark_script::ScriptGame;

static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Runs `source` as main.luau for a few frames, returns the script error (if any).
fn run(source: &str) -> Option<String> {
    let dir =
        std::env::temp_dir().join(format!("spark-strict-{}-{}", std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("main.luau");
    std::fs::write(&path, source).unwrap();
    let mut game = ScriptGame::new(path).hot_reload(false).timeout(Some(std::time::Duration::from_millis(300)));
    let mut world = World::new();
    world.physics.set_backend(Box::new(spark_physics::RapierBackend::new()));
    game.start(&mut world);
    for _ in 0..4 {
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

fn must_fail(source: &str) -> String {
    run(source).unwrap_or_else(|| panic!("expected an error for:\n{source}"))
}

#[test]
fn misspelled_callbacks_are_errors() {
    for (wrong, right) in [
        ("Update", "update"),
        ("fixedUpdate", "fixed_update"),
        ("Render", "render"),
        ("lateUpdate", "late_update"),
        ("Start", "start"),
        ("draw2d", "render"),
        ("updaet", "update"),
        ("FixedUpdate", "fixed_update"),
    ] {
        let e = must_fail(&format!("function {wrong}(dt) end\n"));
        assert!(e.contains(&format!("'{wrong}' is not a Spark callback (did you mean '{right}'?)")), "{wrong}: {e}");
        assert!(e.contains("lowercase snake_case: start, update, fixed_update, late_update, render"), "{e}");
    }
}

#[test]
fn helper_functions_are_not_flagged() {
    let e = run("function updateScore(n) end\nfunction render_ui() end\nfunction startGame() end\nfunction stars() end\n\
         function makeUpdate() end\nfunction update(dt) updateScore(1) end\nfunction helper() end\n");
    assert_eq!(e, None);
}

#[test]
fn misspelled_builtin_events_are_errors() {
    let e = must_fail("on(\"colision\", function() end)\n");
    assert!(e.contains("'colision' is not a built-in event (did you mean 'collision'?)"), "{e}");
    let e = must_fail("once(\"key_press\", function() end)\n");
    assert!(e.contains("did you mean 'key_pressed'"), "{e}");
    let e = must_fail("local o = spawn(Mesh.cube(), \"red\")\no:on(\"collison\", function() end)\n");
    assert!(e.contains("did you mean 'collision'"), "{e}");
    let e = must_fail("on(\"Wheel\", function() end)\n");
    assert!(e.contains("did you mean 'wheel'"), "{e}");
}

#[test]
fn custom_and_builtin_events_work() {
    let e = run("on(\"player_died\", function() end)\non(\"collision\", function() end)\non(\"key_pressed\", function() end)\n\
         once(\"game_over\", function() end)\nlocal o = spawn(Mesh.cube(), \"red\")\no:on(\"collision_end\", function() end)\no:on(\"hit\", function() end)\n");
    assert_eq!(e, None);
}

#[test]
fn unknown_globals_are_errors() {
    let e = must_fail("function update(dt) local x = mth.sin(1) end\n");
    assert!(e.contains("unknown global 'mth' (did you mean 'math'?)"), "{e}");
    let e = must_fail("local x = totally_undefined_thing\n");
    assert!(e.contains("unknown global 'totally_undefined_thing'"), "{e}");
    assert!(!e.contains("did you mean"), "{e}");
}

#[test]
fn globals_still_work() {
    let e = run("score = 0\nfunction update(dt) score = score + 1 end\nassert(rawget(_G, \"nothing\") == nil)\n\
         assert(rawget(_G, \"score\") == 0)\nlater = 5\nassert(later == 5)\nassert(math.sin(0) == 0)\n\
         assert(pcall(function() return nope_nope end) == false)\n");
    assert_eq!(e, None);
}

#[test]
fn unknown_fields_are_errors_with_suggestion() {
    let e = must_fail("local c = spawn(Mesh.cube(), \"red\")\nc.postion = vec3(0, 1, 0)\n");
    assert!(e.contains("unknown field 'postion'") && e.contains("did you mean 'position'?"), "{e}");
    let e = must_fail("local c = spawn(Mesh.cube(), \"red\")\nlocal p = c.postion\n");
    assert!(e.contains("unknown field 'postion'") && e.contains("did you mean 'position'?"), "{e}");
    let e = must_fail("local c = spawn(Mesh.cube(), \"red\")\nc:rotate_yy(1)\n");
    assert!(e.contains("rotate_y"), "{e}");
}

#[test]
fn known_fields_and_methods_are_not_flagged() {
    let e = run("local c = spawn(Mesh.cube(), \"red\")\nc.position = vec3(0, 1, 0)\nc.x = 2\nlocal _ = c.color\nlocal _ = c.body\n\
         local _ = c.name\nc:rotate_y(1)\nassert(c.exists)\n");
    assert_eq!(e, None);
}

#[test]
fn errors_have_no_internal_frames() {
    let e = must_fail("local c = spawn(Mesh.cube(), \"red\")\nc.postion = 1\n");
    assert!(!e.contains("__mlua_"), "{e}");
    assert!(!e.lines().any(|l| l.trim() == "[C]: in ?"), "{e}");
    assert!(e.contains("main.luau:2"), "{e}");
}

#[test]
fn loop_hint_names_task() {
    let e = must_fail("function update(dt) while true do end end\n");
    assert!(e.contains("infinite loop"), "{e}");
    assert!(!e.contains("task.spawn"), "{e}");
    assert!(e.contains("task("), "{e}");
}

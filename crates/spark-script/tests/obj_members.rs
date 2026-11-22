//! The Luau names of `Object` come from the component registry (C-6): every one is readable on an object in the VM and
//! is a candidate for the "did you mean" of an unknown field.

use spark_core::registry::{LuaScope, registry};
use spark_core::{Game, World, run_frame};
use spark_script::{ScriptGame, object_member_names};

/// Every `lua` name of an Object-scoped field (node fields included).
fn registry_names() -> Vec<&'static str> {
    let reg = registry();
    let mut v: Vec<&str> = reg.node_info().fields.iter().filter_map(|f| f.lua).collect();
    for c in reg.components.iter().filter(|c| c.lua == LuaScope::Object) {
        v.extend(c.info().fields.iter().filter_map(|f| f.lua));
    }
    v
}

fn run(source: &str) -> Option<String> {
    static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "spark-objmembers-{}-{}",
        std::process::id(),
        N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("main.luau");
    std::fs::write(&path, source).unwrap();
    let mut game = ScriptGame::new(path).hot_reload(false);
    let mut world = World::new();
    world.physics.set_backend(Box::new(spark_physics::RapierBackend::new()));
    game.start(&mut world);
    for _ in 0..3 {
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

#[test]
fn every_registry_lua_name_is_readable_on_an_object() {
    let names = registry_names();
    assert!(names.len() >= 20, "registry gave only {names:?}");
    // With and without a body: body fields read as defaults on a body-less object.
    for body in [false, true] {
        let mut src = String::from("local o = spawn(Mesh.cube(), \"red\")\n");
        if body {
            src.push_str("o:add_body()\n");
        }
        for n in &names {
            src.push_str(&format!("local _ = o.{n}\n"));
        }
        assert_eq!(run(&src), None, "body = {body}");
    }
}

#[test]
fn every_registry_lua_name_is_a_suggestion_candidate() {
    let candidates = object_member_names();
    for n in registry_names() {
        assert!(candidates.contains(&n), "'{n}' missing from the suggestion candidates");
    }
    // Methods and the non-registry fields stay.
    for n in ["rotate_y", "look_at", "id", "x", "material", "body", "light", "parent"] {
        assert!(candidates.contains(&n), "'{n}' missing from the suggestion candidates");
    }
}

#[test]
fn registry_names_get_suggested_for_typos() {
    for (typo, right) in [("metalic", "metallic"), ("roughnes", "roughness"), ("gravity_scal", "gravity_scale"), ("visibel", "visible")] {
        let e = run(&format!("local o = spawn(Mesh.cube(), \"red\")\nlocal _ = o.{typo}\n")).expect("an error");
        assert!(e.contains(&format!("did you mean '{right}'?")), "{typo}: {e}");
    }
}

/// The generated accessors keep the argument forms and error texts the hand-written ones had (C-7).
#[test]
fn generated_accessors_keep_forms_and_errors() {
    let ok = "local o = spawn(Mesh.cube(), \"red\")\n\
        o.position = vec3(1, 2, 3)\no.position = {x = 1, y = 2, z = 3}\no.position = {4, 5, 6}\n\
        assert(o.position == vec3(4, 5, 6))\no.scale = 2\nassert(o.scale == vec3(2, 2, 2))\no.scale = vec3(1, 2, 3)\n\
        o.rotation = vec3(0.5, 0, 0)\nassert(math.abs(o.rotation.x - 0.5) < 1e-5)\n\
        o.name = \"a\"\nassert(o.name == \"a\")\no.visible = false\nassert(o.visible == false)\no.visible = 1\nassert(o.visible == true)\n";
    assert_eq!(run(ok), None);
    for (stmt, msg) in [
        ("o.position = \"x\"", "position: expected a vector like vec3(1, 2, 3), got string"),
        ("o.rotation = true", "rotation: expected a vector like vec3(1, 2, 3), got boolean"),
        ("o.scale = nil", "scale: expected a vector like vec3(1, 2, 3), got nil"),
        ("o.name = 5", "name: expected a string, got integer"),
        ("o.position = {x = 0/0}", "position: expected a finite number, got NaN"),
    ] {
        let e = run(&format!("local o = spawn(Mesh.cube(), \"red\")\n{stmt}\n")).expect("an error");
        assert!(e.contains(msg), "{stmt}: {e}");
    }
    let e = run("local o = spawn(Mesh.cube(), \"red\")\no:destroy()\no.visible = true\n").expect("an error");
    assert!(e.contains("object was destroyed"), "{e}");
}

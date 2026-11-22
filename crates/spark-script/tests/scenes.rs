//! `scene.load`, `scene.save` and `spawn("x.prefab")` in a real game VM (headless), on files made in a temp folder.

use std::path::Path;

use spark_core::{Game, World, run_frame};
use spark_script::ScriptGame;

const DOOR: &str = r##"[prefab]
format_version = 1
name = "Door"

[[node]]
id = "root"
name = "Door"

[[node]]
id = "frame"
parent = "root"
name = "Frame"

[node.mesh]
primitive = "box"
size = [0.2, 2, 0.2]

[[node]]
id = "leaf"
parent = "root"
name = "Leaf"

[node.transform]
position = [0.5, 1, 0]

[node.mesh]
primitive = "box"
size = [1, 2, 0.1]

[node.material]
color = "#a67c52"
"##;

const ARENA: &str = r##"[scene]
format_version = 1
name = "Arena"

[environment]
background = "#102030"

[[node]]
id = "crate"
name = "Crate"

[node.transform]
position = [0, 0.5, 0]

[node.mesh]
primitive = "box"
size = [1, 1, 1]

[node.material]
color = "#d8a45a"

[[node]]
id = "door1"
name = "Door A"
prefab = "prefabs/door.prefab"

[node.transform]
position = [4, 0, -2]

[node.overrides]
"leaf/material.color" = "#8a3b2f"
"##;

struct Game_ {
    dir: std::path::PathBuf,
}

impl Drop for Game_ {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// A game folder with the level, the prefab and `main.luau` = `script`; runs it until it quits or fails.
fn run(name: &str, script: &str) -> (Game_, World, Option<String>) {
    run_with(name, &[], script)
}

/// [`run`] with more files in the game folder.
fn run_with(name: &str, files: &[(&str, &str)], script: &str) -> (Game_, World, Option<String>) {
    let dir = std::env::temp_dir().join(format!("spark-scenes-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("levels")).unwrap();
    std::fs::create_dir_all(dir.join("prefabs")).unwrap();
    std::fs::write(dir.join("levels/arena.scene"), ARENA).unwrap();
    std::fs::write(dir.join("prefabs/door.prefab"), DOOR).unwrap();
    for (path, text) in files {
        std::fs::write(dir.join(path), text).unwrap();
    }
    std::fs::write(dir.join("main.luau"), script).unwrap();
    let mut game = ScriptGame::new(dir.join("main.luau")).hot_reload(false);
    let mut world = World::new();
    world.physics.set_backend(Box::new(spark_physics::RapierBackend::new()));
    game.start(&mut world);
    for _ in 0..30 {
        if world.quit_requested() || game.error().is_some() {
            break;
        }
        run_frame(&mut game, &mut world, 1.0 / 60.0);
    }
    let error = game.error().map(str::to_string);
    let quit = world.quit_requested();
    drop(game);
    let error = error.or_else(|| (!quit).then(|| "the script did not finish".to_string()));
    (Game_ { dir }, world, error)
}

fn read(dir: &Path, file: &str) -> String {
    std::fs::read_to_string(dir.join(file)).unwrap_or_else(|e| panic!("{file}: {e}"))
}

#[test]
fn load_a_level_spawn_a_prefab_save_and_read_it_back() {
    let (g, world, error) = run(
        "roundtrip",
        r##"
function start()
    local roots = scene.load("levels/arena.scene", { replace = true })
    assert(#roots == 2, "roots: " .. #roots)
    local crate = find("Crate")
    assert(crate ~= nil and crate.position.y == 0.5, "the crate is at its place")
    assert(find("Door A") ~= nil and find("Leaf") ~= nil, "the prefab instance was expanded")
    assert(#find("Door A"):children() == 2)

    local door = spawn("prefabs/door.prefab", { position = vec3(1, 0, 2), name = "Door B" })
    assert(door.position == vec3(1, 0, 2) and door.name == "Door B")
    assert(#door:children() == 2)

    local warnings = scene.save("out/saved.scene")
    assert(#warnings == 0, "warnings: " .. table.concat(warnings, "; "))

    -- Reading the file back gives the same objects.
    local again = scene.load("out/saved.scene", { replace = true })
    assert(#again == 3, "roots after the reload: " .. #again)
    assert(find("Door B").position == vec3(1, 0, 2))
    assert(#find_all("Leaf") == 2)
    quit()
end
"##,
    );
    assert_eq!(error, None);
    let saved = read(&g.dir, "out/saved.scene");
    assert!(saved.contains("prefab = \"prefabs/door.prefab\""), "{saved}");
    assert_eq!(saved.matches("prefab = ").count(), 2, "{saved}");
    assert!(saved.contains("name = \"Door B\"") && saved.contains("position = [1, 0, 2]"), "{saved}");
    assert!(saved.contains("\"leaf/material.color\" = \"#8a3b2f\""), "{saved}");
    assert!(saved.contains("background = \"#102030\""), "the environment is saved:\n{saved}");
    assert!(!saved.contains("Frame") && !saved.contains("Leaf"), "prefab nodes are not written out:\n{saved}");
    assert_eq!(world.scene.len(), 1 + 3 + 3);
}

#[test]
fn a_changed_color_is_saved_as_an_override_and_a_reset_one_disappears() {
    let (g, _, error) = run(
        "overrides",
        r##"
local function close(a, b) return math.abs(a - b) < 1e-3 end

function start()
    scene.load("levels/arena.scene", { replace = true })
    local frame = find("Frame")
    frame.color = Color.hex("#112233")
    scene.save("out/changed.scene")

    frame.color = Color.hex("#a67c52")      -- not the frame's color in the prefab: still an override
    local leaf = find("Leaf")
    leaf.color = Color.hex("#a67c52")       -- the leaf's color in the prefab: back to the prefab's value
    scene.save("out/reset.scene")

    -- The leaf's override went away, so a fresh load shows the prefab's color.
    scene.load("out/reset.scene", { replace = true })
    assert(close(find("Leaf").color.r, 0xa6 / 255), "the leaf has the prefab's color again")
    assert(close(find("Frame").color.b, 0x52 / 255), "the frame keeps its override")
    quit()
end
"##,
    );
    assert_eq!(error, None);
    let changed = read(&g.dir, "out/changed.scene");
    assert!(changed.contains("\"frame/material.color\" = \"#112233\""), "{changed}");
    assert!(changed.contains("\"leaf/material.color\" = \"#8a3b2f\""), "{changed}");
    let reset = read(&g.dir, "out/reset.scene");
    assert!(reset.contains("\"frame/material.color\" = \"#a67c52\""), "{reset}");
    assert!(!reset.contains("leaf/"), "a value reset to the prefab's disappears:\n{reset}");
}

#[test]
fn an_added_child_round_trips() {
    let (g, _, error) = run(
        "child",
        r##"
function start()
    scene.load("levels/arena.scene", { replace = true })
    local door = find("Door A")
    spawn(Mesh.cube(0.2), "red", { name = "Handle", parent = door })
    spawn(Mesh.sphere(0.1), "blue", { name = "Knob", parent = find("Leaf"), position = vec3(0.4, 1, 0) })
    scene.save("out/child.scene")
    scene.load("out/child.scene", { replace = true })
    assert(find("Handle").parent == find("Door A"))
    assert(find("Knob").parent == find("Leaf"))
    assert(#find("Door A"):children() == 3)
    quit()
end
"##,
    );
    assert_eq!(error, None);
    let text = read(&g.dir, "out/child.scene");
    assert!(text.contains("parent = \"door1\""), "{text}");
    assert!(text.contains("parent = \"door1/leaf\""), "{text}");
}

#[test]
fn errors_name_the_file_and_the_paths_stay_in_the_game_folder() {
    let (_g, _, error) = run(
        "errors",
        r##"
local function fails(f, needle)
    local ok, err = pcall(f)
    assert(not ok, "expected an error mentioning " .. needle)
    assert(string.find(tostring(err), needle, 1, true), "the error does not mention '" .. needle .. "': " .. tostring(err))
end

function start()
    fails(function() scene.load("levels/nope.scene") end, "levels/nope.scene")
    fails(function() spawn("prefabs/nope.prefab") end, "prefabs/nope.prefab")
    fails(function() scene.load("../outside.scene") end, "game folder")
    fails(function() scene.load("/etc/x.scene") end, "absolute")
    fails(function() scene.save("../outside.scene") end, "game folder")
    fails(function() scene.save("levels/a.txt") end, "must be a .scene file")
    fails(function() scene.load("prefabs/door.prefab") end, "spawn(")
    fails(function() scene.load("levels/arena.scene", { replce = true }) end, "unknown option 'replce'")
    fails(function() spawn("prefabs/door.prefab", "red") end, "table of properties")
    fails(function() spawn("prefabs/door.prefab", { colour = "red" }) end, "did you mean 'color'")
    assert(#find_all("Door") == 0, "a failed spawn leaves nothing behind")

    -- A level that does not load leaves the current scene alone.
    spawn(Mesh.cube(), "red", { name = "Keep" })
    fails(function() scene.load("levels/arena.scene", { replace = "yes" }) end, "replace")
    assert(find("Keep") ~= nil)
    quit()
end
"##,
    );
    assert_eq!(error, None);
}

#[test]
fn a_broken_file_reports_the_line() {
    let dir = std::env::temp_dir().join(format!("spark-scenes-broken-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("levels")).unwrap();
    std::fs::write(dir.join("levels/bad.scene"), "[scene]\nformat_version = 1\n\n[[node]]\nid = \"a\"\n\n[node.bodyy]\n").unwrap();
    std::fs::write(
        dir.join("main.luau"),
        "function start()\n  local ok, err = pcall(function() scene.load(\"levels/bad.scene\") end)\n  assert(not ok)\n  err = tostring(err)\n  assert(string.find(err, \"levels/bad.scene\", 1, true), err)\n  assert(string.find(err, \"line 7\", 1, true), err)\n  assert(string.find(err, \"did you mean\", 1, true), err)\n  quit()\nend\n",
    )
    .unwrap();
    let mut game = ScriptGame::new(dir.join("main.luau")).hot_reload(false);
    let mut world = World::new();
    game.start(&mut world);
    for _ in 0..5 {
        if world.quit_requested() || game.error().is_some() {
            break;
        }
        run_frame(&mut game, &mut world, 1.0 / 60.0);
    }
    let error = game.error().map(str::to_string);
    let quit = world.quit_requested();
    drop(game);
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(error, None);
    assert!(quit);
}

const CAM_LEVEL: &str = r##"[scene]
format_version = 1

[[node]]
id = "cam0"
name = "Cam"
tags = ["view"]

[node.transform]
position = [1, 2, 3]

[node.camera]
fov = 40.0

[[node]]
id = "crate"
name = "Crate"
tags = ["pickup", "wood"]

[node.mesh]
primitive = "box"
size = [1, 1, 1]
"##;

#[test]
fn camera_global_drives_the_camera_node_and_the_file() {
    let (g, _world, error) = run_with(
        "camnode",
        &[("levels/cam.scene", CAM_LEVEL)],
        r##"
function start()
    scene.load("levels/cam.scene", { replace = true })
    assert(camera.fov == 40, "the camera follows the node: " .. camera.fov)
    assert(camera.position == vec3(1, 2, 3), "position from the node")
    camera.position = vec3(5, 6, 7)
    camera.fov = 75
    local node = find("Cam")
    assert(node.position == vec3(5, 6, 7), "camera.position moved the node")
    node.position = vec3(0, 1, 0)
    -- a node moved by a script is picked up at the next frame stage
    wait(0.05)
    assert(camera.position == vec3(0, 1, 0), "the node moved the camera")
    camera.position = vec3(5, 6, 7)
    scene.save("levels/out.scene")
    quit()
end
"##,
    );
    assert_eq!(error, None);
    let out = read(&g.dir, "levels/out.scene");
    assert!(out.contains("position = [5, 6, 7]"), "{out}");
    assert!(out.contains("fov = 75.0"), "{out}");
    assert!(out.contains(r#"tags = ["pickup", "wood"]"#), "{out}");
}

#[test]
fn tags_find_save_and_prefab_override() {
    let prefab = format!("{}\n", DOOR.replace("name = \"Leaf\"", "name = \"Leaf\"\ntags = [\"wood\"]"));
    let (g, _world, error) = run_with(
        "tags",
        &[("levels/cam.scene", CAM_LEVEL), ("prefabs/door.prefab", &prefab)],
        r##"
function start()
    scene.load("levels/cam.scene", { replace = true })
    local crate = find("Crate")
    assert(crate:has_tag("wood") and not crate:has_tag("steel"))
    assert(#crate.tags == 2 and crate.tags[1] == "pickup")
    crate.tags[1] = "x" -- a copy: nothing changes
    assert(crate:has_tag("pickup"))
    crate:add_tag("heavy")
    crate:add_tag("heavy")
    assert(#crate.tags == 3)
    assert(crate:remove_tag("wood") == true and crate:remove_tag("wood") == false)
    assert(#find_all_tagged("pickup") == 1 and #find_all_tagged("nothing") == 0)
    crate.tags = { "a", "b" }
    assert(#find_all_tagged("a") == 1 and not crate:has_tag("heavy"))
    local ok, err = pcall(function() crate:add_tag("") end)
    assert(not ok and string.find(tostring(err), "empty"), tostring(err))
    local door = spawn("prefabs/door.prefab", { name = "Door" })
    local leaf = door:find("Leaf")
    assert(leaf:has_tag("wood"), "tags come from the prefab")
    leaf:add_tag("opens")
    scene.save("levels/out.scene")
    scene.load("levels/out.scene", { replace = true })
    assert(find("Crate"):has_tag("b"), "tags round-trip")
    assert(find("Leaf"):has_tag("opens") and find("Leaf"):has_tag("wood"), "the override round-trips")
    quit()
end
"##,
    );
    assert_eq!(error, None);
    let out = read(&g.dir, "levels/out.scene");
    assert!(out.contains(r#"tags = ["a", "b"]"#), "{out}");
    assert!(out.contains(r#""leaf/node.tags" = ["wood", "opens"]"#), "{out}");
}

//! Prefab instances: expanded when a scene loads, collapsed (diffed against a fresh instance) when it is saved.

use spark_core::World;
use spark_scene::{PrefabSource, load_scene, parse, save_scene, spawn_prefab};

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

const LEVEL: &str = r##"[scene]
format_version = 1

[[node]]
id = "floor"
name = "Floor"

[node.mesh]
primitive = "plane"
width = 10.0
depth = 10.0

[[node]]
id = "door1"
name = "Door A"
prefab = "prefabs/door.prefab"

[node.transform]
position = [4, 0, -2]

[node.overrides]
"leaf/transform.rotation" = [0, 90, 0]
"leaf/material.color" = "#8a3b2f"
"##;

/// A prefab source over strings (the tests need no files).
fn files(list: &'static [(&'static str, &'static str)]) -> impl PrefabSource {
    move |path: &str| match list.iter().find(|(p, _)| *p == path) {
        Some((_, text)) => parse(text).map(|l| l.file).map_err(|e| format!("'{path}': {e}")),
        None => Err(format!("cannot read '{path}': no such file")),
    }
}

fn source() -> impl PrefabSource {
    files(&[("prefabs/door.prefab", DOOR)])
}

fn load(world: &mut World, text: &str, replace: bool) {
    let file = parse(text).unwrap().file;
    load_scene(world, &file, &mut source(), replace).unwrap();
}

#[test]
fn an_instance_is_expanded_and_the_overrides_applied() {
    let mut w = World::new();
    load(&mut w, LEVEL, true);
    assert_eq!(w.scene.len(), 4, "floor + door root + frame + leaf");
    let leaf = w.scene.find("Leaf").unwrap();
    let c = w.scene[leaf].material.color;
    assert_eq!((c.r, c.g, c.b), (0x8a as f32 / 255.0, 0x3b as f32 / 255.0, 0x2f as f32 / 255.0));
    assert!((w.scene[leaf].transform.rotation.to_euler(spark_core::glam::EulerRot::XYZ).1.to_degrees() - 90.0).abs() < 1e-3);
    let door = w.scene.find("Door A").unwrap();
    assert_eq!(w.scene[door].transform.position.x, 4.0);
    assert_eq!(w.scene.children(door).len(), 2);
    // The frame keeps what the prefab says.
    let frame = w.scene.find("Frame").unwrap();
    assert!(w.scene[frame].mesh.is_some());
}

#[test]
fn an_unchanged_instance_saves_to_the_same_text() {
    let mut w = World::new();
    load(&mut w, LEVEL, true);
    let saved = save_scene(&mut w, &mut source()).unwrap();
    assert_eq!(saved.text, LEVEL);
    assert!(saved.warnings.is_empty(), "{:?}", saved.warnings);
}

#[test]
fn a_changed_color_lands_in_the_file_and_a_reset_one_disappears() {
    let mut w = World::new();
    load(&mut w, LEVEL, true);
    let leaf = w.scene.find("Leaf").unwrap();
    let frame = w.scene.find("Frame").unwrap();
    // Changed: the frame gets a new color, the leaf keeps its override.
    w.scene[frame].material.color = spark_core::Color::hex(0x336699);
    let text = save_scene(&mut w, &mut source()).unwrap().text;
    assert!(text.contains("\"frame/material.color\" = \"#336699\""), "{text}");
    assert!(text.contains("\"leaf/material.color\" = \"#8a3b2f\""), "{text}");
    assert_eq!(text.matches("material.color").count(), 2, "only the changed values are written:\n{text}");

    // Reset to the prefab's values: the lines go away (the leaf's rotation too), and the file is the plain instance.
    w.scene[frame].material.color = spark_core::Color::WHITE;
    w.scene[leaf].material.color = parse(DOOR).map(|_| spark_core::Color::hex(0xa67c52)).unwrap();
    w.scene[leaf].transform.rotation = spark_core::glam::Quat::IDENTITY;
    let text = save_scene(&mut w, &mut source()).unwrap().text;
    assert!(!text.contains("overrides"), "{text}");
    assert!(!text.contains("material"), "{text}");
    assert!(text.contains("prefab = \"prefabs/door.prefab\""), "{text}");

    // And it reads back to the same objects.
    let mut again = World::new();
    load(&mut again, &text, true);
    let leaf = again.scene.find("Leaf").unwrap();
    assert_eq!(again.scene[leaf].material.color, spark_core::Color::hex(0xa67c52));
}

#[test]
fn a_child_added_to_an_instance_round_trips() {
    let level = format!(
        "{LEVEL}\n[[node]]\nid = \"knob\"\nparent = \"door1/leaf\"\nname = \"Knob\"\n\n[node.transform]\nposition = [0.4, 1, 0]\n\n[[node]]\nid = \"lamp\"\nparent = \"door1\"\nname = \"Lamp\"\n"
    );
    let mut w = World::new();
    load(&mut w, &level, true);
    let (door, lamp, knob, leaf) =
        (w.scene.find("Door A").unwrap(), w.scene.find("Lamp").unwrap(), w.scene.find("Knob").unwrap(), w.scene.find("Leaf").unwrap());
    assert_eq!(w.scene[lamp].parent(), Some(door));
    assert_eq!(w.scene[knob].parent(), Some(leaf));
    assert_eq!(w.scene.len(), 6);
    let saved = save_scene(&mut w, &mut source()).unwrap();
    assert_eq!(saved.text, level);
    assert!(saved.warnings.is_empty(), "{:?}", saved.warnings);
}

#[test]
fn spawning_a_prefab_makes_an_instance_that_saves_as_one() {
    let mut w = World::new();
    let (root, warnings) = spawn_prefab(&mut w, "prefabs/door.prefab", &mut source()).unwrap();
    assert!(warnings.is_empty());
    assert_eq!(w.scene[root].name, "Door");
    assert_eq!(w.scene.len(), 3);
    w.scene[root].transform.position = spark_core::glam::Vec3::new(1.0, 0.0, 2.0);
    let (second, _) = spawn_prefab(&mut w, "prefabs/door.prefab", &mut source()).unwrap();
    assert_ne!(root, second);
    assert_eq!(w.scene.len(), 6, "two instances, no id clashes");
    let text = save_scene(&mut w, &mut source()).unwrap().text;
    assert_eq!(text.matches("prefab = \"prefabs/door.prefab\"").count(), 2, "{text}");
    assert!(text.contains("position = [1, 0, 2]"), "{text}");
    assert!(!text.contains("overrides"), "{text}");
}

#[test]
fn removed_prefab_nodes_are_reported_not_saved() {
    let mut w = World::new();
    load(&mut w, LEVEL, true);
    let frame = w.scene.find("Frame").unwrap();
    w.scene.destroy(frame);
    let saved = save_scene(&mut w, &mut source()).unwrap();
    assert_eq!(saved.warnings.len(), 1, "{:?}", saved.warnings);
    assert!(saved.warnings[0].contains("'frame' was removed"), "{:?}", saved.warnings);
    assert_eq!(saved.text, LEVEL, "the file still says what the prefab says");
}

#[test]
fn a_missing_prefab_names_the_file() {
    let file = parse(LEVEL).unwrap().file;
    let mut w = World::new();
    let e = load_scene(&mut w, &file, &mut files(&[]), true).unwrap_err().to_string();
    assert!(e.contains("prefabs/door.prefab") && e.contains("door1") && e.contains("no such file"), "{e}");
    assert!(w.scene.is_empty(), "nothing was created");
}

#[test]
fn bad_overrides_and_prefabs_are_clear_errors() {
    let run = |level: &str, prefab: &'static str| {
        let file = parse(level).unwrap().file;
        let mut w = World::new();
        let leaked: &'static [(&'static str, &'static str)] = Box::leak(Box::new([("prefabs/door.prefab", prefab)]));
        load_scene(&mut w, &file, &mut files(leaked), true).unwrap_err().to_string()
    };
    let e = run(&LEVEL.replace("leaf/transform", "lef/transform"), DOOR);
    assert!(e.contains("the prefab has no node 'lef'") && e.contains("root, frame, leaf"), "{e}");

    let nested = format!("{DOOR}\n[[node]]\nid = \"inner\"\nparent = \"root\"\nprefab = \"x.prefab\"\n");
    let e = run(LEVEL, Box::leak(nested.into_boxed_str()));
    assert!(e.contains("nested prefabs are not supported"), "{e}");

    let two_roots = format!("{DOOR}\n[[node]]\nid = \"second\"\n");
    let e = run(LEVEL, Box::leak(two_roots.into_boxed_str()));
    assert!(e.contains("exactly one root"), "{e}");

    let e = run(LEVEL, LEVEL);
    assert!(e.contains("not a prefab"), "{e}");

    // A parent inside a node that is not an instance.
    let e = run(&LEVEL.replace("[[node]]\nid = \"door1\"", "[[node]]\nid = \"door1\"\nparent = \"floor/x\""), DOOR);
    assert!(e.contains("needs 'floor' to be a prefab instance"), "{e}");
}

#[test]
fn override_keys_are_checked_when_the_file_is_read() {
    for (key, needle) in [
        ("\"leaf/material.colour\"", "did you mean `color`"),
        ("\"leaf/materal.color\"", "did you mean"),
        ("\"material.color\"", "must look like"),
        ("\"leaf/transform.position.x\"", "override the whole field"),
        ("\"leaf/node\"", "write \"leaf/node.name\""),
        ("\"leaf/mesh.size\"", "a mesh is overridden whole"),
    ] {
        let text = LEVEL.replace("\"leaf/material.color\"", key);
        let e = parse(&text).unwrap_err().to_string();
        assert!(e.contains(needle), "{key}: {e}");
        assert!(e.contains("line "), "{key}: {e}");
    }
}

#[test]
fn whole_component_overrides_and_node_fields_round_trip() {
    let level = r##"[scene]
format_version = 1

[[node]]
id = "door1"
prefab = "prefabs/door.prefab"

[node.overrides]
"frame/node.name" = "Post"
"frame/node.visible" = false
"frame/body" = { mass = 2.0 }
"leaf/mesh" = { primitive = "sphere", radius = 0.5, segments = 12 }
"##;
    let mut w = World::new();
    load(&mut w, level, true);
    let post = w.scene.find("Post").unwrap();
    assert!(!w.scene[post].visible);
    assert_eq!(w.scene[post].body.as_ref().unwrap().mass, Some(2.0));
    let saved = save_scene(&mut w, &mut source()).unwrap();
    assert_eq!(saved.text, level);
}

#[test]
fn loading_twice_renames_ids_and_keeps_the_environment() {
    let nodes = LEVEL.split_once("[[node]]").map(|(_, rest)| format!("[[node]]{rest}")).unwrap();
    let file = parse(&format!("[scene]\nformat_version = 1\n\n[environment]\nbackground = \"#102030\"\n\n{nodes}")).unwrap().file;
    let mut w = World::new();
    let first = load_scene(&mut w, &file, &mut source(), true).unwrap();
    assert_eq!(first.roots.len(), 2);
    assert_eq!(w.scene.background, spark_core::Color::hex(0x102030));
    w.scene.background = spark_core::Color::BLACK;
    let second = load_scene(&mut w, &file, &mut source(), false).unwrap();
    assert_eq!(second.roots.len(), 2);
    assert_eq!(w.scene.len(), 8);
    assert!(second.warnings.iter().any(|m| m.contains("renamed")), "{:?}", second.warnings);
    assert_eq!(w.scene.background, spark_core::Color::BLACK, "an additive load keeps the environment");
    // The ids stay unique through a save.
    let text = save_scene(&mut w, &mut source()).unwrap().text;
    assert!(parse(&text).is_ok(), "{text}");
    // replace: starts over.
    load_scene(&mut w, &file, &mut source(), true).unwrap();
    assert_eq!(w.scene.len(), 4);
}

#[test]
fn a_failed_replace_keeps_the_old_scene() {
    let mut w = World::new();
    load(&mut w, LEVEL, true);
    let broken = parse(&LEVEL.replace("prefabs/door.prefab", "prefabs/gone.prefab")).unwrap().file;
    assert!(load_scene(&mut w, &broken, &mut source(), true).is_err());
    assert_eq!(w.scene.len(), 4);
}

/// The files of `examples/08-scenes` are canonical (a save gives the same bytes) and the level saves back to itself.
#[test]
fn the_scenes_example_is_canonical() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/08-scenes");
    for file in ["levels/arena.scene", "prefabs/door.prefab"] {
        let text = std::fs::read_to_string(dir.join(file)).unwrap().replace("\r\n", "\n");
        let again = spark_scene::to_string(&parse(&text).unwrap().file).unwrap();
        assert_eq!(again, text, "{file} is not in the canonical form");
    }
    let mut loader = spark_scene::FsLoader::new(&dir, true);
    let level = loader.read("levels/arena.scene").unwrap();
    let mut w = World::new();
    load_scene(&mut w, &level, &mut loader, true).unwrap();
    assert_eq!(w.scene.len(), 5);
    let saved = save_scene(&mut w, &mut loader).unwrap();
    let text = std::fs::read_to_string(dir.join("levels/arena.scene")).unwrap().replace("\r\n", "\n");
    // The saved file has no header comment (comments are not kept through a world), the rest is the same.
    assert_eq!(saved.text, text.lines().skip(1).collect::<Vec<_>>().join("\n") + "\n");
}

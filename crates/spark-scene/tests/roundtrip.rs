//! Scene files: the golden files of ADR 0003 round-trip byte for byte, a code-built world survives
//! capture -> save -> load -> instantiate -> capture, mistakes are reported with line numbers, old formats migrate.

use spark_core::doc::{capture, instantiate};
use spark_core::glam::{Quat, Vec3};
use spark_core::{Body, Color, Fog, Light, Material, MeshData, MeshSource, Sky, TextureId, World};
use spark_scene::{ErrorKind, FORMAT_VERSION, parse, read_version, to_string, to_string_lossy};

const ARENA: &str = include_str!("data/arena.scene");
const DOOR: &str = include_str!("data/door.prefab");
const LEGACY: &str = include_str!("data/legacy_v0.scene");
const LEGACY_MIGRATED: &str = include_str!("data/legacy_v0.migrated.scene");

/// A scene with the three header lines, so the first line of `body` is line 4.
fn scene(body: &str) -> String {
    format!("[scene]\nformat_version = 1\n\n{body}")
}

fn err(text: &str) -> spark_scene::SceneError {
    parse(text).expect_err("the text should be rejected")
}

// ---------------------------------------------------------------------------------------------------------
// Golden files

#[test]
fn golden_scene_round_trips_byte_for_byte() {
    let loaded = parse(ARENA).unwrap();
    assert_eq!(loaded.version, FORMAT_VERSION);
    assert!(!loaded.migrated());
    assert_eq!(loaded.file.nodes.len(), 4);
    assert_eq!(loaded.file.name, "Arena");
    assert_eq!(loaded.file.comment, vec!["# Arena: first test level".to_string()]);
    assert_eq!(to_string(&loaded.file).unwrap(), ARENA);
}

#[test]
fn golden_prefab_round_trips_byte_for_byte() {
    let loaded = parse(DOOR).unwrap();
    assert_eq!(loaded.file.kind, spark_scene::FileKind::Prefab);
    assert_eq!(loaded.file.nodes.len(), 3);
    assert_eq!(to_string(&loaded.file).unwrap(), DOOR);
}

/// The examples in ADR 0003 are the golden files (the ADR must not drift away from what the writer produces).
#[test]
fn the_adr_examples_are_the_golden_files() {
    let adr = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/dev/adr/0003-scene-and-prefab-format.md"))
        .unwrap()
        .replace("\r\n", "\n");
    let block = |after: &str| {
        let from = adr.find(after).unwrap_or_else(|| panic!("the ADR has no section {after}"));
        let rest = &adr[from..];
        let start = rest.find("```toml\n").unwrap() + "```toml\n".len();
        let end = rest[start..].find("```").unwrap();
        rest[start..start + end].to_string()
    };
    assert_eq!(block("### Example `levels/arena.scene`"), ARENA);
    assert_eq!(block("### Example `prefabs/door.prefab`"), DOOR);
}

#[test]
fn golden_scene_is_read_correctly() {
    let file = parse(ARENA).unwrap().file;
    let lamp = file.nodes.iter().find(|n| n.id.as_str() == "lamp").unwrap();
    // Spot angle: degrees in the file, radians in the registry.
    let light = lamp.component("light").unwrap();
    let kind = light.field("kind").unwrap();
    let angle = kind.field("angle").unwrap();
    assert!(matches!(angle, spark_reflect::Value::Float(a) if (*a as f32 - 50f32.to_radians()).abs() < 1e-6), "{angle:?}");
    // Colors are exact bytes / 255.
    assert_eq!(file.environment.background, Color::hex(0x141a1f));
    // Defaults are filled in.
    let crate_ = file.nodes.iter().find(|n| n.id.as_str() == "crate").unwrap();
    assert_eq!(crate_.component("body").unwrap().field("friction"), Some(&spark_reflect::Value::Float(0.5)));
    assert_eq!(crate_.tags, ["pickup", "wood"]);
    // Prefab instance: override values are kept as written.
    let door = file.nodes.iter().find(|n| n.id.as_str() == "door1").unwrap();
    assert_eq!(door.prefab.as_deref(), Some("prefabs/door.prefab"));
    assert_eq!(door.overrides.len(), 2);
}

#[test]
fn golden_scene_instantiates() {
    let file = parse(ARENA).unwrap().file;
    let mut w = World::new();
    let r = instantiate(&file, &mut w).unwrap();
    assert_eq!(r.count, 4);
    // camera, tags, prefab and overrides are not interpreted yet: reported, not silent.
    assert!(r.warnings.iter().any(|m| m.contains("camera")), "{:?}", r.warnings);
    assert!(r.warnings.iter().any(|m| m.contains("tags")), "{:?}", r.warnings);
    assert!(r.warnings.iter().any(|m| m.contains("prefab")), "{:?}", r.warnings);
    let c = w.scene.find("Crate").unwrap();
    let body = w.scene[c].body.as_ref().unwrap();
    assert_eq!(body.mass, Some(4.0));
    assert_eq!(w.scene[c].material.roughness, 0.6);
    assert!(w.scene[c].material.pbr);
    assert!(w.scene[c].mesh.is_some());
    let lamp = w.scene.find("Lamp").unwrap();
    let light = w.scene[lamp].light.unwrap();
    assert_eq!(light.color, Color::hex(0xffe9c4));
    assert!(matches!(light.kind, spark_core::LightKind::Spot { angle, .. } if angle == 50f32.to_radians()));
    assert_eq!(w.scene.sun.intensity, 1.2);
    assert_eq!(w.scene[w.scene.find("Camera").unwrap()].transform.position, Vec3::new(0.0, 3.0, 8.0));
}

// ---------------------------------------------------------------------------------------------------------
// World -> file -> world

fn build_world() -> World {
    let mut w = World::new();
    w.scene.ambient = Color::rgb(0.1, 0.2, 0.3); // not a byte / 255: written as floats
    w.scene.background = Color::hex(0x202830);
    w.scene.fog = Some(Fog { near: 5.0, ..Fog::default() });
    w.scene.sky = Some(Sky::default().with_lighting(0.5));
    w.scene.sun.intensity = 2.5;

    let boxed =
        w.assets.add_mesh_from(&MeshSource::Box { size: Vec3::new(1.0, 2.0, 3.0), tile: Some(0.5), cell: None, surface: true }).unwrap();
    let ball = w.assets.add_mesh_from(&MeshSource::Sphere { radius: 0.25, segments: 8 }).unwrap();
    let crate_ = w.spawn(boxed, Material::color(Color::hex(0xd8a45a)).with_roughness(0.6));
    w.scene[crate_].name = "Crate \"A\"".into();
    w.scene[crate_].transform.position = Vec3::new(1.0, 0.1, -2.5);
    w.scene[crate_].transform.rotation = Quat::from_euler(spark_core::EulerRot::YXZ, 30f32.to_radians(), 10f32.to_radians(), 0.0);
    w.scene[crate_].transform.scale = Vec3::new(1.0, 2.0, 1.0);
    w.scene[crate_].lod.push((20.0, ball));
    w.scene[crate_].body = Some(Body { mass: Some(4.0), bounciness: 0.2, ..Body::default() });

    let lamp = w.spawn_empty();
    w.scene[lamp].name = "Lamp".into();
    w.scene[lamp].light = Some(Light::spot(Color::hex(0xffe9c4), 3.0, 12.0, 50f32.to_radians(), 0.3));
    w.scene[lamp].visible = false;
    w.scene.set_parent(lamp, Some(crate_)).unwrap();

    let tri = w.assets.add_mesh_from(&MeshSource::Triangles { points: vec![Vec3::ZERO, Vec3::X, Vec3::Y] }).unwrap();
    let t = w.spawn(tri, Material::unlit(Color::rgba(1.0, 0.5, 0.25, 0.5)));
    w.scene[t].name = "Tri".into();
    w.scene.set_parent(t, Some(lamp)).unwrap();

    let sibling = w.spawn_empty();
    w.scene[sibling].name = "Second root".into();
    w
}

#[test]
fn code_built_world_round_trips() {
    let mut w = build_world();
    let doc = capture(&mut w);
    assert_eq!(doc.nodes.len(), 4);
    let text = to_string(&doc).unwrap();

    let loaded = parse(&text).unwrap();
    assert_eq!(loaded.file, doc, "load(save(capture)) == capture\n{text}");

    let mut w2 = World::new();
    let r = instantiate(&loaded.file, &mut w2).unwrap();
    assert!(r.warnings.is_empty(), "{:?}", r.warnings);
    assert_eq!(capture(&mut w2), doc, "capture(instantiate(doc)) == doc");
    assert_eq!(to_string(&capture(&mut w2)).unwrap(), text, "saving again gives the same bytes");

    // The things the text shows.
    assert!(text.contains("ambient = [0.1, 0.2, 0.3, 1]"), "{text}");
    assert!(text.contains("background = \"#202830\""));
    assert!(text.contains("name = \"Crate \\\"A\\\"\""));
    assert!(text.contains("visible = false"));
    assert!(text.contains("rotation = [10, 30, 0]"), "{text}");
    assert!(text.contains("lod = [{ distance = 20.0, primitive = \"sphere\", radius = 0.25, segments = 8 }]"), "{text}");
    assert!(text.contains("color = \"#ff804080\"") || text.contains("color = [1, 0.5, 0.25, 0.5]"), "{text}");
    assert!(text.contains("kind = { type = \"spot\", angle = 50.0 }"), "{text}");
    assert!(!text.contains("velocity") && !text.contains("impulse"), "runtime-only fields are never written");
    assert!(text.contains("pbr = true") && text.contains("roughness = 0.6"), "{text}");

    // Hierarchy survives: the lamp is under the crate, the triangle under the lamp.
    let crate_ = w2.scene.find("Crate \"A\"").unwrap();
    let lamp = w2.scene.find("Lamp").unwrap();
    assert_eq!(w2.scene[lamp].parent(), Some(crate_));
    assert_eq!(w2.scene[w2.scene.find("Tri").unwrap()].parent(), Some(lamp));
    assert_eq!(w2.scene[crate_].lod.len(), 1);
    assert_eq!(w2.scene.fog, w.scene.fog);
}

#[test]
fn ids_are_kept_through_a_round_trip() {
    let mut w = World::new();
    let a = w.spawn_empty();
    let id = w.node_id(a).unwrap();
    let text = to_string(&capture(&mut w)).unwrap();
    assert!(text.contains(&format!("id = \"{id}\"")));
    let mut w2 = World::new();
    instantiate(&parse(&text).unwrap().file, &mut w2).unwrap();
    assert!(w2.find_node(&id).is_some());
}

// ---------------------------------------------------------------------------------------------------------
// What cannot be saved

#[test]
fn runtime_only_meshes_are_reported_not_dropped() {
    let mut w = World::new();
    let id = w.add_mesh(MeshData::cube(1.0)); // made by code: no recipe
    let o = w.spawn(id, Material::default());
    w.scene[o].name = "Custom".into();
    let doc = capture(&mut w);
    let node_id = doc.nodes[0].id.clone();

    let e = to_string(&doc).unwrap_err();
    assert_eq!(e.issues.len(), 1);
    assert_eq!(e.issues[0].node.as_ref(), Some(&node_id));
    assert_eq!(e.issues[0].component, "mesh");
    let shown = e.to_string();
    assert!(shown.contains(node_id.as_str()) && shown.contains("Custom") && shown.contains("recipe"), "{shown}");

    let (text, issues) = to_string_lossy(&doc);
    assert_eq!(issues, e.issues);
    assert!(!text.contains("[node.mesh]"));
    assert!(text.contains("name = \"Custom\""), "the node itself is saved");
}

#[test]
fn texture_handles_are_reported() {
    let mut w = World::new();
    let o = w.spawn_empty();
    w.scene[o].material.texture = Some(TextureId(3));
    let e = to_string(&capture(&mut w)).unwrap_err();
    assert_eq!(e.issues.len(), 1);
    assert_eq!(e.issues[0].component, "material");
    assert!(e.issues[0].message.contains("texture"), "{}", e.issues[0]);
}

// ---------------------------------------------------------------------------------------------------------
// Mistakes in a file

#[test]
fn duplicate_ids_name_the_line() {
    let e = err(&scene("[[node]]\nid = \"a\"\n\n[[node]]\nid = \"a\"\n"));
    assert_eq!(e.kind, ErrorKind::Structure);
    assert_eq!(e.line, Some(8), "{e}");
    assert!(e.message.contains("duplicate") && e.message.contains("'a'"), "{e}");
    assert!(e.to_string().starts_with("line 8, column 6:"), "{e}");
}

#[test]
fn dangling_parents_name_the_line() {
    let e = err(&scene("[[node]]\nid = \"a\"\n\n[[node]]\nid = \"b\"\nparent = \"nope\"\n"));
    assert_eq!(e.kind, ErrorKind::Structure);
    assert_eq!(e.line, Some(9), "{e}");
    assert!(e.message.contains("'nope'"), "{e}");
}

#[test]
fn parent_cycles_are_rejected() {
    let e = err(&scene("[[node]]\nid = \"a\"\nparent = \"b\"\n\n[[node]]\nid = \"b\"\nparent = \"a\"\n"));
    assert_eq!(e.kind, ErrorKind::Structure);
    assert!(e.message.contains("ancestor"), "{e}");
    assert!(e.line.is_some());
}

#[test]
fn unknown_fields_come_with_a_suggestion_and_a_line() {
    let e = err(&scene("[[node]]\nid = \"crate\"\n\n[node.body]\nmasss = 4.0\n"));
    assert_eq!(e.kind, ErrorKind::UnknownName);
    assert_eq!(e.line, Some(8), "{e}");
    assert!(e.message.contains("`masss`") && e.message.contains("did you mean `mass`"), "{e}");
    assert!(e.to_string().contains("node 'crate'") && e.to_string().contains("[node.body]"), "{e}");
}

#[test]
fn unknown_components_and_keys_come_with_a_suggestion() {
    let e = err(&scene("[[node]]\nid = \"a\"\n\n[node.bodyy]\nmass = 1.0\n"));
    assert_eq!((e.kind, e.line), (ErrorKind::UnknownName, Some(7)));
    assert!(e.message.contains("did you mean `body`"), "{e}");
    let e = err(&scene("[[node]]\nid = \"a\"\nnme = \"x\"\n"));
    assert!(e.message.contains("did you mean `name`"), "{e}");
    let e = err(&scene("[[node]]\nid = \"a\"\n\n[node.mesh]\nprimitive = \"bax\"\n"));
    assert!(e.message.contains("did you mean `box`") && e.line == Some(8), "{e}");
    let e = err(&scene("[[node]]\nid = \"a\"\n\n[node.body]\nkind = \"dynamc\"\n"));
    assert!(e.message.contains("did you mean `dynamic`") && e.line == Some(8), "{e}");
    let e = err("[scene]\nformat_version = 1\n[nodes]\n");
    assert!(e.message.contains("did you mean `node`") && e.line == Some(3), "{e}");
}

#[test]
fn wrong_types_name_the_line() {
    let e = err(&scene("[[node]]\nid = \"a\"\n\n[node.body]\nmass = \"heavy\"\n"));
    assert_eq!((e.kind, e.line), (ErrorKind::Invalid, Some(8)));
    assert!(e.message.contains("`mass`") && e.message.contains("expected a number, found a string"), "{e}");
    let e = err(&scene("[[node]]\nid = \"a\"\n\n[node.transform]\nposition = [1, 2]\n"));
    assert_eq!(e.line, Some(8));
    assert!(e.message.contains("3 numbers"), "{e}");
    let e = err(&scene("[[node]]\nid = \"a\"\n\n[node.material]\ncolor = \"red\"\n"));
    assert_eq!(e.line, Some(8));
    assert!(e.message.contains("#rrggbb"), "{e}");
    let e = err(&scene("[[node]]\nid = \"a\"\nvisible = 1\n"));
    assert_eq!(e.line, Some(6));
}

#[test]
fn values_out_of_range_are_rejected_at_their_line() {
    let e = err(&scene("[[node]]\nid = \"a\"\n\n[node.body]\nmass = -1.0\n"));
    assert_eq!((e.kind, e.line), (ErrorKind::Invalid, Some(8)));
    assert!(e.message.contains("out of range"), "{e}");
    let e = err(&scene("[[node]]\nid = \"a\"\n\n[node.light]\nkind = { type = \"spot\", softnes = 0.5 }\n"));
    assert_eq!(e.line, Some(8));
    assert!(e.message.contains("did you mean `softness`"), "{e}");
}

#[test]
fn runtime_state_cannot_be_written_in_a_file() {
    let e = err(&scene("[[node]]\nid = \"a\"\n\n[node.body]\nvelocity = [0, 1, 0]\n"));
    assert_eq!(e.line, Some(8));
    assert!(e.message.contains("runtime"), "{e}");
}

#[test]
fn syntax_errors_have_a_line() {
    let e = err(&scene("[[node]]\nid = \n"));
    assert_eq!(e.kind, ErrorKind::Syntax);
    assert!(e.line.is_some(), "{e}");
    let e = err("name = \"no header\"\n");
    assert_eq!(e.kind, ErrorKind::Invalid);
}

#[test]
fn bad_ids_are_rejected() {
    let e = err(&scene("[[node]]\nid = \"Has Space\"\n"));
    assert_eq!(e.line, Some(5));
    let e = err(&scene("[[node]]\nname = \"no id\"\n"));
    assert!(e.message.contains("needs an `id`"), "{e}");
}

// ---------------------------------------------------------------------------------------------------------
// Versions

#[test]
fn an_old_file_is_migrated_on_load() {
    assert_eq!(read_version(LEGACY).unwrap(), 0);
    let loaded = parse(LEGACY).unwrap();
    assert_eq!(loaded.version, 0);
    assert!(loaded.migrated());
    // The v0 spelling `{ kind = "spot" }` was understood.
    let light = loaded.file.nodes[0].component("light").unwrap();
    assert!(matches!(light.field("kind"), Some(spark_reflect::Value::Enum { variant, .. }) if variant == "spot"));
    // ... and saving writes the current version, canonically.
    assert_eq!(to_string(&loaded.file).unwrap(), LEGACY_MIGRATED);
}

#[test]
fn migrate_text_rewrites_old_files_and_leaves_current_ones() {
    let m = spark_scene::migrate_text(LEGACY).unwrap();
    assert_eq!((m.from, m.to), (0, FORMAT_VERSION));
    assert_eq!(m.text, LEGACY_MIGRATED);
    // Migrating the result changes nothing.
    let again = spark_scene::migrate_text(&m.text).unwrap();
    assert_eq!((again.from, again.to), (FORMAT_VERSION, FORMAT_VERSION));
    assert_eq!(again.text, m.text);
    // A current file that is not canonical is not rewritten either.
    let messy = "[scene]\nformat_version=1\n[[node]]\nid='a'\n";
    assert_eq!(spark_scene::migrate_text(messy).unwrap().text, messy);
}

#[test]
fn a_file_from_the_future_is_rejected_with_a_clear_message() {
    let future = format!("[scene]\nformat_version = {}\n\n[[node]]\nid = \"a\"\nsome_new_key = 1\n", FORMAT_VERSION + 1);
    for e in [err(&future), read_version(&future).unwrap_err()] {
        assert_eq!(e.kind, ErrorKind::TooNew);
        assert_eq!(e.line, Some(2));
        let m = e.to_string();
        assert!(m.contains(&format!("format_version {}", FORMAT_VERSION + 1)) && m.contains("update Spark"), "{m}");
    }
}

#[test]
fn a_missing_or_malformed_version_is_rejected() {
    let e = err("[scene]\nname = \"x\"\n");
    assert!(e.message.contains("format_version"), "{e}");
    let e = err("[scene]\nformat_version = \"1\"\n");
    assert_eq!(e.line, Some(2));
    let e = err("[scene]\nformat_version = -1\n");
    assert_eq!(e.line, Some(2));
}

#[test]
fn errors_in_a_migrated_file_say_so() {
    let e = err("[scene]\nformat_version = 0\n\n[[node]]\nid = \"a\"\n\n[node.body]\nmasss = 1.0\n");
    assert!(e.message.contains("did you mean `mass`") && e.message.contains("migrating"), "{e}");
}

// ---------------------------------------------------------------------------------------------------------
// Odds and ends of the format

#[test]
fn spellings_are_accepted_and_written_canonically() {
    // Other number spellings, a short body, tables in another order, ids in any valid form.
    let text = scene(
        "[[node]]\nname = \"Z\"\nid = \"door_leaf-2\"\n\n[node.material]\nroughness = 1\npbr = true\ncolor = [0.25, 0.5, 1.0, 1.0]\n\n\
         [node.transform]\nrotation = [0, 90.00001, -0.0]\nposition = [1e2, 0, 0.5]\n",
    );
    let doc = parse(&text).unwrap().file;
    let canon = to_string(&doc).unwrap();
    assert_eq!(
        canon,
        "[scene]\nformat_version = 1\n\n[[node]]\nid = \"door_leaf-2\"\nname = \"Z\"\n\n[node.transform]\nposition = [100, 0, 0.5]\n\
         rotation = [0, 90, 0]\n\n[node.material]\ncolor = [0.25, 0.5, 1, 1]\npbr = true\nroughness = 1.0\n"
    );
    assert_eq!(to_string(&parse(&canon).unwrap().file).unwrap(), canon);
}

#[test]
fn default_components_are_written_as_empty_tables_when_optional() {
    let mut w = World::new();
    let o = w.spawn_empty();
    w.scene[o].body = Some(Body::default());
    w.scene[o].light = Some(Light::default());
    let text = to_string(&capture(&mut w)).unwrap();
    assert!(text.contains("[node.body]\n") && text.contains("[node.light]"), "{text}");
    assert!(
        !text.contains("[node.transform]") && !text.contains("[node.material]"),
        "always-present components at defaults are omitted\n{text}"
    );
    let mut w2 = World::new();
    instantiate(&parse(&text).unwrap().file, &mut w2).unwrap();
    let o2 = w2.scene.iter().next().unwrap().0;
    assert_eq!(w2.scene[o2].body, Some(Body::default()));
    assert_eq!(w2.scene[o2].light, Some(Light::default()));
}

#[test]
fn model_meshes_use_part_for_the_primitive_index() {
    let text = scene("[[node]]\nid = \"m\"\n\n[node.mesh]\nasset = \"assets/door.glb\"\nnode = \"Frame\"\npart = 2\n");
    let doc = parse(&text).unwrap().file;
    assert_eq!(to_string(&doc).unwrap(), text);
    let e = err(&scene("[[node]]\nid = \"m\"\n\n[node.mesh]\nasset = \"a.glb\"\nnode = \"N\"\nprimitive = 1\n"));
    assert!(e.message.contains("part"), "{e}");
    let e = err(&scene("[[node]]\nid = \"m\"\n\n[node.mesh]\nsize = [1, 1, 1]\n"));
    assert!(e.message.contains("primitive"), "{e}");
}

#[test]
fn the_guid_form_of_an_asset_reference_is_read() {
    let text = scene("[[node]]\nid = \"m\"\n\n[node.mesh]\nasset = { guid = \"x\", path = \"assets/a.glb\" }\nnode = \"N\"\n");
    // The reader accepts the table form for fields typed as assets; the writer writes the path.
    let file = parse(&text).unwrap().file;
    let mesh = file.nodes[0].component("mesh").unwrap();
    assert_eq!(mesh.field("asset"), Some(&spark_reflect::Value::Asset("assets/a.glb".into())));
}

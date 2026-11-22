//! Snapshot of the error texts and side effects of the material, body and light fields of `Object` (C-8, C-9).
//!
//! These were captured from the hand-written closures BEFORE the fields were generated from the component registry;
//! the generated accessors must keep every line of it.

use spark_core::{Game, World, run_frame};
use spark_script::ScriptGame;

fn run(source: &str) -> Option<String> {
    static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let dir =
        std::env::temp_dir().join(format!("spark-binderr-{}-{}", std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
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

/// First line of the message: `main.luau:N: text` -> `text`.
fn message(e: &str) -> String {
    let line = e.lines().next().unwrap_or("");
    let line = line.strip_prefix("runtime error: ").unwrap_or(line);
    match line.find(".luau:") {
        Some(at) => {
            let rest = &line[at + 6..];
            rest.split_once(": ").map(|(_, m)| m).unwrap_or(rest).to_string()
        }
        None => line.to_string(),
    }
}

const SETUP: &str = "local o = spawn(Mesh.cube(), \"red\")\nlocal b = spawn(Mesh.cube(), \"red\", { body = \"dynamic\" })\n\
    local l = spawn(Mesh.cube(), \"red\", { light = \"spot\" })\n";

fn failing(stmt: &str) -> String {
    let e = run(&format!("{SETUP}{stmt}\n")).unwrap_or_else(|| panic!("`{stmt}` did not fail"));
    message(&e)
}

#[test]
fn print_snapshot() {
    if std::env::var("SPARK_PRINT_SNAPSHOT").is_err() {
        return;
    }
    for stmt in STATEMENTS {
        let e = run(&format!("{SETUP}{stmt}\n"));
        println!("SNAP {stmt:?} => {:?}", e.map(|e| message(&e)));
    }
}

const STATEMENTS: &[&str] = &[
    "o.color = \"nope\"",
    "o.color = true",
    "o.color = {r = \"x\"}",
    "o.color = 0/0",
    "o.blend = \"bogus\"",
    "o.blend = 5",
    "o.blend = nil",
    "o.double_sided = \"x\"",
    "o.metallic = \"x\"",
    "o.metallic = nil",
    "o.metallic = 0/0",
    "o.metallic = 1e300",
    "o.roughness = {}",
    "o.roughness = \"rough\"",
    "o.emissive = \"nope\"",
    "o.emissive = false",
    "o.data = \"x\"",
    "o.data = {x = \"s\"}",
    "o.data = {1, 2, 3, 0/0}",
    "o.normal_map = 5",
    "o.shader = 5",
    "b.velocity = \"x\"",
    "b.velocity = vec3(0/0, 0, 0)",
    "o.velocity = vec3(1, 2, 3)",
    "o.angular_velocity = vec3(1, 2, 3)",
    "o.angular_velocity = \"x\"",
    "o.mass = 5",
    "b.mass = -1",
    "b.mass = 0",
    "b.mass = \"x\"",
    "o.gravity_scale = 2",
    "b.gravity_scale = \"x\"",
    "o.sensor = true",
    "l.light.intensity = \"x\"",
    "l.light.range = {}",
    "l.light.color = \"nope\"",
    "l.light.type = \"x\"",
    "l.light.type = 5",
    "l.light.shadows = 1",
    "l.light.angle = \"x\"",
    "local p = spawn(Mesh.cube(), \"red\", { light = \"point\" }); p.light.angle = 30",
    "local p = spawn(Mesh.cube(), \"red\", { light = \"point\" }); p.light.softness = 0.5",
    "local k = l.light; l.light = nil; k.intensity = 1",
    "local k = l.light; l.light = nil; local _ = k.intensity",
    "l.light.bogus = 1",
    "local _ = l.light.bogus",
    "o:destroy(); o.color = \"red\"",
    "o:destroy(); local _ = o.metallic",
    "o:destroy(); o.velocity = vec3(1, 1, 1)",
    "spawn(Mesh.cube(), \"red\", { metallic = \"x\" })",
    "spawn(Mesh.cube(), \"red\", { roughness = true })",
    "spawn(Mesh.cube(), \"red\", { blend = \"bogus\" })",
    "spawn(Mesh.cube(), \"red\", { color = \"nope\" })",
    "spawn(Mesh.cube(), \"red\", { visible = 1 })",
    "spawn(Mesh.cube(), \"red\", { double_sided = 1 })",
    "spawn(Mesh.cube(), \"red\", { cast_shadow = \"yes\" })",
    "spawn(Mesh.cube(), \"red\", { pbr = 0 })",
    "spawn(Mesh.cube(), \"red\", { emissive = \"nope\" })",
    "spawn(Mesh.cube(), \"red\", { data = \"x\" })",
    "spawn(Mesh.cube(), \"red\", { colour = \"red\" })",
    "spawn(Mesh.cube(), \"red\", { mass = 5 })",
    "spawn(Mesh.cube(), \"red\", { velocity = vec3(1, 0, 0) })",
    "spawn(Mesh.cube(), \"red\", { x = 1 })",
    "spawn(Mesh.cube(), \"red\", { metalic = 1 })",
    "light({ intensty = 2 })",
    "light({ colour = \"red\" })",
];

#[test]
fn side_effects_of_material_fields() {
    let src = format!(
        "{SETUP}\
        assert(not o.pbr)\n\
        o.metallic = 0.5\nassert(o.pbr and o.metallic == 0.5, 'metallic turns pbr on')\n\
        o.pbr = false\nassert(not o.pbr and o.metallic == 0.5, 'pbr can be turned off again')\n\
        o.roughness = 2\nassert(o.pbr and o.roughness == 1, 'roughness clamps and turns pbr on')\n\
        o.metallic = -3\nassert(o.metallic == 0)\n\
        o.pbr = nil\nassert(o.pbr == false)\n\
        o.blend = 'transparent'\nassert(o.blend == 'alpha')\n\
        o.blend = 'add'\nassert(o.blend == 'additive')\n\
        o.blend = 'opaque'\nassert(o.blend == 'opaque')\n\
        o.double_sided = 1\nassert(o.double_sided == true)\n\
        o.cast_shadow = nil\nassert(o.cast_shadow == false)\n\
        o.color = '#ff0000'\nassert(o.color == Color.rgb(1, 0, 0))\n\
        o.emissive = '#0000ff'\nassert(o.emissive == Color.rgb(0, 0, 1))\n\
        o.data = vec3(1, 2, 3)\nassert(o.data.x == 1 and o.data.y == 2 and o.data.z == 3 and o.data.w == 0)\n\
        o.data = 7\nassert(o.data.x == 7 and o.data.y == 0)\n\
        local s = spawn(Mesh.cube(), 'red', {{ metallic = 0.3, roughness = 0.2, blend = 'add', double_sided = true, cast_shadow = false, emissive = 'red', data = {{1, 2}} }})\n\
        assert(s.pbr and math.abs(s.metallic - 0.3) < 1e-6 and math.abs(s.roughness - 0.2) < 1e-6 and s.blend == 'additive' and s.double_sided and not s.cast_shadow)\n\
        assert(s.data.x == 1 and s.data.y == 2)\n\
        local q = spawn(Mesh.cube(), 'red', {{ pbr = true }})\nassert(q.pbr and q.metallic == 0)\n"
    );
    assert_eq!(run(&src), None);
}

#[test]
fn side_effects_of_body_and_light_fields() {
    let src = format!(
        "{SETUP}\
        assert(o.velocity == vec3(0, 0, 0) and o.angular_velocity == vec3(0, 0, 0))\n\
        assert(o.mass == nil and o.gravity_scale == nil and o.sensor == false)\n\
        b.velocity = vec3(1, 2, 3)\nassert(b.velocity == vec3(1, 2, 3))\n\
        b.angular_velocity = {{1, 2, 3}}\nassert(b.angular_velocity == vec3(1, 2, 3))\n\
        b.mass = 4\nassert(b.mass == 4)\nb.mass = nil\nassert(b.mass == nil)\n\
        b.gravity_scale = 0.5\nassert(b.gravity_scale == 0.5)\n\
        b.sensor = 1\nassert(b.sensor == true)\nb.sensor = nil\nassert(b.sensor == false)\n\
        assert(l.light.type == 'spot')\n\
        l.light.intensity = -1\nassert(l.light.intensity == 0)\n\
        l.light.range = 0\nassert(math.abs(l.light.range - 0.01) < 1e-6)\n\
        l.light.angle = 500\nassert(math.abs(l.light.angle - 179) < 1e-3)\n\
        l.light.softness = 7\nassert(l.light.softness == 1)\n\
        l.light.shadows = true\nassert(l.light.shadows == true)\n\
        l.light.color = '#0000ff'\nassert(l.light.color == Color.rgb(0, 0, 1))\n\
        l.light.type = 'point'\nassert(l.light.type == 'point' and l.light.angle == nil and l.light.shadows == true)\n\
        local s = spawn(Mesh.cube(), 'red', {{ light = {{ type = 'spot', angle = 60, intensity = 2 }}, body = {{ mass = 3, velocity = vec3(0, 1, 0) }} }})\n\
        assert(s.light.intensity == 2 and s.mass == 3 and s.velocity == vec3(0, 1, 0))\n"
    );
    assert_eq!(run(&src), None);
}

#[test]
fn error_texts_are_unchanged() {
    for (stmt, expected) in SNAPSHOT {
        assert_eq!(&failing(stmt), expected, "{stmt}");
    }
}

/// `(statement, first line of the error without `main.luau:N:`)`, from the hand-written closures.
const SNAPSHOT: &[(&str, &str)] = &[
    (
        "o.color = \"nope\"",
        "color: unknown color 'nope' (use \"#rrggbb\", 0xrrggbb or a name: white, black, gray, grey, red, green, blue, yellow, orange, purple, pink, brown, sky)",
    ),
    ("o.color = true", "color: expected a color like \"#ff8800\" or \"red\", got boolean"),
    ("o.color = {r = \"x\"}", "color: expected a number, got string"),
    ("o.color = 0/0", "color: expected a finite number, got NaN (0/0 or math.sqrt(-1) somewhere?)"),
    ("o.blend = \"bogus\"", "blend: unknown blend mode 'bogus' (allowed: auto, opaque, alpha, additive)"),
    ("o.blend = 5", "blend: expected a string, got integer"),
    ("o.blend = nil", "blend: expected a string, got nil"),
    ("o.metallic = \"x\"", "bad argument #2 to `Obj.metallic`: runtime error: argument: expected a number, got string"),
    ("o.metallic = nil", "bad argument #2 to `Obj.metallic`: runtime error: argument: expected a number, got nil"),
    (
        "o.metallic = 0/0",
        "bad argument #2 to `Obj.metallic`: runtime error: argument: expected a finite number, got NaN (0/0 or math.sqrt(-1) somewhere?)",
    ),
    ("o.metallic = 1e300", "bad argument #2 to `Obj.metallic`: runtime error: argument: number is too large (max about 3.4e38)"),
    ("o.roughness = {}", "bad argument #2 to `Obj.roughness`: runtime error: argument: expected a number, got table"),
    ("o.roughness = \"rough\"", "bad argument #2 to `Obj.roughness`: runtime error: argument: expected a number, got string"),
    (
        "o.emissive = \"nope\"",
        "emissive: unknown color 'nope' (use \"#rrggbb\", 0xrrggbb or a name: white, black, gray, grey, red, green, blue, yellow, orange, purple, pink, brown, sky)",
    ),
    ("o.emissive = false", "emissive: expected a color like \"#ff8800\" or \"red\", got boolean"),
    ("o.data = \"x\"", "data: expected a number, got string"),
    ("o.data = {x = \"s\"}", "data: expected a number, got string"),
    ("o.data = {1, 2, 3, 0/0}", "data: expected a finite number, got NaN (0/0 or math.sqrt(-1) somewhere?)"),
    ("o.normal_map = 5", "normal_map: expected a Texture or an image path like \"player.png\", got integer"),
    ("o.shader = 5", "shader: expected a Shader (Shader.load(\"shaders/x.wgsl\")) or nil, got integer"),
    ("b.velocity = \"x\"", "obj.velocity: expected a vector like vec3(1, 2, 3), got string"),
    (
        "b.velocity = vec3(0/0, 0, 0)",
        "bad argument #1: runtime error: argument: expected a finite number, got NaN (0/0 or math.sqrt(-1) somewhere?)",
    ),
    ("o.velocity = vec3(1, 2, 3)", "obj.velocity: object has no body (add one with obj:add_body() or spawn(..., { body = \"dynamic\" }))"),
    (
        "o.angular_velocity = vec3(1, 2, 3)",
        "obj.angular_velocity: object has no body (add one with obj:add_body() or spawn(..., { body = \"dynamic\" }))",
    ),
    ("o.angular_velocity = \"x\"", "obj.angular_velocity: expected a vector like vec3(1, 2, 3), got string"),
    ("o.mass = 5", "obj.mass: object has no body (add one with obj:add_body() or spawn(..., { body = \"dynamic\" }))"),
    ("b.mass = -1", "obj.mass: must be greater than 0, got -1"),
    ("b.mass = 0", "obj.mass: must be greater than 0, got 0"),
    ("b.mass = \"x\"", "bad argument #2 to `Obj.mass`: runtime error: argument: expected a number, got string"),
    ("o.gravity_scale = 2", "obj.gravity_scale: object has no body (add one with obj:add_body() or spawn(..., { body = \"dynamic\" }))"),
    ("b.gravity_scale = \"x\"", "bad argument #2 to `Obj.gravity_scale`: runtime error: argument: expected a number, got string"),
    ("o.sensor = true", "obj.sensor: object has no body (add one with obj:add_body() or spawn(..., { body = \"dynamic\" }))"),
    ("l.light.intensity = \"x\"", "light.intensity: expected a number, got string"),
    ("l.light.range = {}", "light.range: expected a number, got table"),
    (
        "l.light.color = \"nope\"",
        "light.color: unknown color 'nope' (use \"#rrggbb\", 0xrrggbb or a name: white, black, gray, grey, red, green, blue, yellow, orange, purple, pink, brown, sky)",
    ),
    ("l.light.type = \"x\"", "light: unknown light type 'x' (use \"point\" or \"spot\")"),
    ("l.light.type = 5", "light.type: expected a string, got integer"),
    ("l.light.shadows = 1", "light.shadows: expected true/false, got integer"),
    ("l.light.angle = \"x\"", "light.angle: expected a number, got string"),
    (
        "local p = spawn(Mesh.cube(), \"red\", { light = \"point\" }); p.light.angle = 30",
        "light.angle: only spot lights have 'angle' (set type = \"spot\")",
    ),
    (
        "local p = spawn(Mesh.cube(), \"red\", { light = \"point\" }); p.light.softness = 0.5",
        "light.softness: only spot lights have 'softness' (set type = \"spot\")",
    ),
    ("local k = l.light; l.light = nil; k.intensity = 1", "light was removed from this object"),
    ("local k = l.light; l.light = nil; local _ = k.intensity", "light was removed from this object"),
    ("l.light.bogus = 1", "attempt to set an unknown field 'bogus'"),
    ("local _ = l.light.bogus", "attempt to get an unknown field 'bogus'"),
    ("o:destroy(); o.color = \"red\"", "object was destroyed (check obj:exists() before using it)"),
    ("o:destroy(); local _ = o.metallic", "object was destroyed (check obj:exists() before using it)"),
    ("o:destroy(); o.velocity = vec3(1, 1, 1)", "object was destroyed (check obj:exists() before using it)"),
    ("spawn(Mesh.cube(), \"red\", { metallic = \"x\" })", "property 'metallic': expected a number, got string"),
    ("spawn(Mesh.cube(), \"red\", { roughness = true })", "property 'roughness': expected a number, got boolean"),
    (
        "spawn(Mesh.cube(), \"red\", { blend = \"bogus\" })",
        "property 'blend': unknown blend mode 'bogus' (allowed: auto, opaque, alpha, additive)",
    ),
    (
        "spawn(Mesh.cube(), \"red\", { color = \"nope\" })",
        "property 'color': unknown color 'nope' (use \"#rrggbb\", 0xrrggbb or a name: white, black, gray, grey, red, green, blue, yellow, orange, purple, pink, brown, sky)",
    ),
    ("spawn(Mesh.cube(), \"red\", { visible = 1 })", "property 'visible': expected true/false, got integer"),
    ("spawn(Mesh.cube(), \"red\", { double_sided = 1 })", "property 'double_sided': expected true/false, got integer"),
    ("spawn(Mesh.cube(), \"red\", { cast_shadow = \"yes\" })", "property 'cast_shadow': expected true/false, got string"),
    ("spawn(Mesh.cube(), \"red\", { pbr = 0 })", "property 'pbr': expected true/false, got integer"),
    (
        "spawn(Mesh.cube(), \"red\", { emissive = \"nope\" })",
        "property 'emissive': unknown color 'nope' (use \"#rrggbb\", 0xrrggbb or a name: white, black, gray, grey, red, green, blue, yellow, orange, purple, pink, brown, sky)",
    ),
    ("spawn(Mesh.cube(), \"red\", { data = \"x\" })", "property 'data': expected a number, got string"),
    (
        "spawn(Mesh.cube(), \"red\", { colour = \"red\" })",
        "unknown property 'colour' (did you mean 'color'?) (allowed: name, visible, position, rotation, scale, color, data, blend, double_sided, cast_shadow, pbr, metallic, roughness, emissive, parent, shader, lod, normal_map, orm_map, body, light)",
    ),
    (
        "spawn(Mesh.cube(), \"red\", { mass = 5 })",
        "unknown property 'mass' (allowed: name, visible, position, rotation, scale, color, data, blend, double_sided, cast_shadow, pbr, metallic, roughness, emissive, parent, shader, lod, normal_map, orm_map, body, light)",
    ),
    (
        "spawn(Mesh.cube(), \"red\", { velocity = vec3(1, 0, 0) })",
        "unknown property 'velocity' (allowed: name, visible, position, rotation, scale, color, data, blend, double_sided, cast_shadow, pbr, metallic, roughness, emissive, parent, shader, lod, normal_map, orm_map, body, light)",
    ),
    (
        "spawn(Mesh.cube(), \"red\", { x = 1 })",
        "unknown property 'x' (allowed: name, visible, position, rotation, scale, color, data, blend, double_sided, cast_shadow, pbr, metallic, roughness, emissive, parent, shader, lod, normal_map, orm_map, body, light)",
    ),
    (
        "spawn(Mesh.cube(), \"red\", { metalic = 1 })",
        "unknown property 'metalic' (did you mean 'metallic'?) (allowed: name, visible, position, rotation, scale, color, data, blend, double_sided, cast_shadow, pbr, metallic, roughness, emissive, parent, shader, lod, normal_map, orm_map, body, light)",
    ),
    (
        "light({ intensty = 2 })",
        "unknown property 'intensty' (allowed: name, visible, position, rotation, scale, color, data, blend, double_sided, cast_shadow, pbr, metallic, roughness, emissive, parent, shader, lod, normal_map, orm_map, body, light)",
    ),
    (
        "light({ colour = \"red\" })",
        "unknown property 'colour' (did you mean 'color'?) (allowed: name, visible, position, rotation, scale, color, data, blend, double_sided, cast_shadow, pbr, metallic, roughness, emissive, parent, shader, lod, normal_map, orm_map, body, light)",
    ),
];

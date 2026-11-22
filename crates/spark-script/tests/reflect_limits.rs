//! The ranges declared on the reflected engine data (`spark-core`, ADR 0004) are the limits the Luau API enforces today.
//! When someone changes a check in the scripting API or an attribute in `spark-core`, one of the two sides fails here.

use spark_core::{Body, Camera, Game, Light, LightKind, Material, Shape, World, run_frame};
use spark_reflect::{Reflect, ReflectEnum, TypeInfo};
use spark_script::ScriptGame;

fn range(info: &'static TypeInfo, name: &str) -> (f64, f64) {
    info.field(name).unwrap().range.unwrap_or_else(|| panic!("{}.{name} has no range", info.name))
}

fn run(source: &str) -> Option<String> {
    let dir = std::env::temp_dir().join(format!("spark-limits-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("main.luau");
    std::fs::write(&path, source).unwrap();
    let mut game = ScriptGame::new(path).hot_reload(false);
    let mut world = World::new();
    game.start(&mut world);
    run_frame(&mut game, &mut world, 1.0 / 60.0);
    let e = game.error().map(str::to_string);
    drop(game);
    let _ = std::fs::remove_dir_all(&dir);
    e
}

#[test]
fn declared_ranges_match_the_luau_checks() {
    let (fov_lo, fov_hi) = range(Camera::type_info(), "fov");
    let (near_lo, _) = range(Camera::type_info(), "near");
    let (far_lo, _) = range(Camera::type_info(), "far");
    let (mass_lo, _) = range(Body::type_info(), "mass");
    let (ortho_lo, _) = range(Camera::type_info(), "ortho");
    let (rad_lo, _) = Shape::enum_info().variant("sphere").unwrap().fields[0].range.unwrap();
    let spot = LightKind::enum_info().variant("spot").unwrap();
    let angle = spot.fields.iter().find(|x| x.name == "angle").unwrap().range.unwrap();
    let (range_lo, _) = range(Light::type_info(), "range");
    let (metal_lo, metal_hi) = range(Material::type_info(), "metallic");
    // Where the declared range excludes 0 ("greater than 0") the scripts must reject 0 and accept the bound.
    assert!(near_lo > 0.0 && far_lo > 0.0 && mass_lo > 0.0 && rad_lo > 0.0);

    let src = format!(
        r#"
local function check(what, f, want)
    local got = pcall(f)
    if got ~= want then error(what .. ": accepted=" .. tostring(got) .. ", expected " .. tostring(want), 0) end
end
local function near(what, got, want)
    if math.abs(got - want) > 1e-4 then error(what .. ": got " .. got .. ", expected " .. want, 0) end
end
local o = spawn(Mesh.cube(), "red")
o:add_body()

-- camera.fov: an error outside the range, nothing is clamped
check("fov below", function() camera.fov = {fov_lo} - 0.5 end, false)
check("fov low", function() camera.fov = {fov_lo} end, true)
check("fov high", function() camera.fov = {fov_hi} end, true)
check("fov above", function() camera.fov = {fov_hi} + 0.5 end, false)
-- near / far: greater than 0
check("near 0", function() camera.near = 0 end, false)
check("near min", function() camera.near = {near_lo:e} end, true)
check("far 0", function() camera.far = 0 end, false)
-- body: mass greater than 0 (nil = computed), friction 0 or more, radius / height greater than 0
check("mass 0", function() o.mass = 0 end, false)
check("mass min", function() o.mass = {mass_lo:e} end, true)
check("mass nil", function() o.mass = nil end, true)
check("friction -", function() o:add_body({{ friction = -0.1 }}) end, false)
check("friction 0", function() o:add_body({{ friction = 0 }}) end, true)
check("radius 0", function() o:add_body({{ shape = "sphere", radius = 0 }}) end, false)
check("radius min", function() o:add_body({{ shape = "sphere", radius = {rad_lo:e} }}) end, true)
check("height 0", function() o:add_body({{ shape = "capsule", height = 0 }}) end, false)
-- clamped, not rejected
o.metallic = {metal_hi} + 5
near("metallic high", o.metallic, {metal_hi})
o.metallic = {metal_lo} - 5
near("metallic low", o.metallic, {metal_lo})
o.roughness = 9
near("roughness", o.roughness, 1)
camera.ortho = 0
near("ortho", camera.ortho, {ortho_lo})
local lamp = light({{ type = "spot", range = 0.0001, intensity = -4, angle = 0.1, softness = 7 }})
near("light range", lamp.light.range, {range_lo})
near("light intensity", lamp.light.intensity, 0)
near("spot angle", lamp.light.angle, {angle_lo_deg})
near("spot softness", lamp.light.softness, 1)
"#,
        angle_lo_deg = angle.0.to_degrees(),
    );
    if let Some(e) = run(&src) {
        panic!("{e}");
    }
}

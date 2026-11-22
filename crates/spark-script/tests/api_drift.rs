//! The API registry (`docs/spark.d.luau`, parsed by `spark-api`) and the running engine must describe the same API.
//!
//! The test builds a Luau script from the registry, runs it in a real game VM and fails with the full list of
//! differences: functions that are documented but missing, and globals / table members that exist but are not documented.

mod common;

use std::fmt::Write;

use spark_api::{Api, Member};
use spark_core::{Game, World, run_frame};
use spark_script::ScriptGame;

/// Luau's own globals (not part of Spark's API).
const STDLIB: &[&str] = &[
    "_G",
    "_VERSION",
    "assert",
    "bit32",
    "buffer",
    "collectgarbage",
    "coroutine",
    "error",
    "gcinfo",
    "getfenv",
    "getmetatable",
    "ipairs",
    "loadstring",
    "math",
    "newproxy",
    "next",
    "os",
    "pairs",
    "pcall",
    "print",
    "rawequal",
    "rawget",
    "rawlen",
    "rawset",
    "require",
    "select",
    "setfenv",
    "setmetatable",
    "string",
    "table",
    "tonumber",
    "tostring",
    "type",
    "typeof",
    "unpack",
    "utf8",
    "vector",
    "xpcall",
    // added by the mlua Luau build, not by Spark
    "integer",
];

/// Tables that exist only in some contexts.
const SKIP_TABLES: &[&str] = &["test"];

/// How to get one instance of each class (empty = the class cannot be created without external files / audio).
fn instance(class: &str) -> Option<&'static str> {
    Some(match class {
        "Color" => "Color.rgb(1, 0, 0)",
        "Mesh" => "Mesh.cube()",
        "Texture" => "Texture.solid(\"red\")",
        "Material" => "Material.pbr(\"red\", 0.1, 0.5)",
        "Model" => "Model.load(\"assets/rig.glb\")",
        "Timer" => "after(100, function() end)",
        "Task" => "task(function() wait(100) end)",
        "Listener" => "on(\"drift_check\", function() end)",
        "Object" => "spawn(Mesh.cube(), \"red\")",
        "Camera" => "camera",
        _ => return None,
    })
}

fn lua_str(s: &str) -> String {
    format!("{s:?}")
}

fn script(api: &Api, only_test_table: bool) -> String {
    let mut s = String::from("local problems = {}\nlocal function bad(m) problems[#problems + 1] = m end\n");
    s.push_str("local function get(t, k) local ok, v = pcall(function() return t[k] end) return ok, v end\n");
    // known stdlib + registry globals
    let _ = write!(s, "local known = {{");
    for n in STDLIB {
        let _ = write!(s, "[{}] = true, ", lua_str(n));
    }
    for n in api.global_names() {
        let _ = write!(s, "[{}] = true, ", lua_str(n));
    }
    if only_test_table {
        s.push_str("[\"start\"] = true, "); // the game's own start() is visible to the test
    }
    s.push_str("}\nfor k in pairs(_G) do if not known[k] then bad('global `' .. tostring(k) .. '` exists but is not in docs/spark.d.luau') end end\n");

    for f in api.functions.iter().filter(|_| !only_test_table) {
        let _ = writeln!(
            s,
            "if type(_G[{n}]) ~= 'function' then bad('global function `{raw}` is documented but missing') end",
            n = lua_str(&f.name),
            raw = f.name
        );
    }
    for v in api.values.iter().filter(|_| !only_test_table) {
        let _ = writeln!(
            s,
            "if _G[{n}] == nil then bad('global value `{raw}` is documented but missing') end",
            n = lua_str(&v.name),
            raw = v.name
        );
    }
    for ns in api.namespaces.iter().filter(|n| if only_test_table { n.name == "test" } else { !SKIP_TABLES.contains(&n.name.as_str()) }) {
        let n = &ns.name;
        let _ = writeln!(s, "do local t = _G[{q}] if t == nil then bad('table `{n}` is documented but missing') else", q = lua_str(n));
        let mut names = String::new();
        for m in &ns.members {
            let _ = write!(names, "[{}] = true, ", lua_str(m.name()));
            match m {
                Member::Function(f) => {
                    let _ = writeln!(
                        s,
                        "local ok, v = get(t, {q}) if not ok or type(v) ~= 'function' then bad('`{n}.{raw}` is documented as a function but is ' .. (ok and type(v) or 'unreadable')) end",
                        q = lua_str(&f.name),
                        raw = f.name
                    );
                }
                Member::Field(f) => {
                    if !f.optional() {
                        let _ = writeln!(
                            s,
                            "local ok, v = get(t, {q}) if not ok or v == nil then bad('`{n}.{raw}` is documented but missing') end",
                            q = lua_str(&f.name),
                            raw = f.name
                        );
                    }
                }
            }
        }
        // Reverse direction only for plain tables (userdata namespaces cannot be enumerated).
        let _ = writeln!(
            s,
            "if type(t) == 'table' then local doc = {{{names}}} for k in pairs(t) do if not doc[k] then bad('`{n}.' .. tostring(k) .. '` exists but is not documented') end end end end end"
        );
    }
    for c in api.classes.iter().filter(|_| !only_test_table) {
        let Some(make) = instance(&c.name) else {
            let _ = writeln!(s, "-- class {} has no instance in this test", c.name);
            continue;
        };
        let _ = writeln!(
            s,
            "do local ok, inst = pcall(function() return {make} end) if not ok or inst == nil then bad('could not create a {} for the drift check: ' .. tostring(inst)) else",
            c.name
        );
        for m in &c.methods {
            let _ = writeln!(
                s,
                "local ok, v = get(inst, {q}) if not ok or type(v) ~= 'function' then bad('`{c}:{raw}` is documented but is ' .. (ok and type(v) or tostring(v))) end",
                q = lua_str(&m.name),
                c = c.name,
                raw = m.name
            );
        }
        for f in c.fields.iter().filter(|f| f.name != "[string]") {
            let _ = writeln!(
                s,
                "local ok, v = get(inst, {q}) if not ok then bad('`{c}.{raw}` is documented but reading it fails: ' .. tostring(v)) end",
                q = lua_str(&f.name),
                c = c.name,
                raw = f.name
            );
        }
        s.push_str("end end\n");
    }
    s.push_str("function start()\n  if #problems > 0 then table.sort(problems) error('API drift (' .. #problems .. '):\\n  ' .. table.concat(problems, '\\n  ')) end\n  quit()\nend\n");
    s
}

#[test]
fn registry_matches_the_running_engine() {
    let api = spark_api::api();
    let dir = std::env::temp_dir().join(format!("spark-api-drift-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("assets")).unwrap();
    common::write_test_rig(&dir.join("assets/rig.glb"));
    let main = dir.join("main.luau");
    std::fs::write(&main, script(&api, false)).unwrap();

    let mut game = ScriptGame::new(main.clone()).hot_reload(false);
    let mut world = World::new();
    world.physics.set_backend(Box::new(spark_physics::RapierBackend::new()));
    game.start(&mut world);
    for _ in 0..5 {
        if world.quit_requested() || game.error().is_some() {
            break;
        }
        run_frame(&mut game, &mut world, 1.0 / 60.0);
    }
    let err = game.error().map(str::to_string);
    let quit = world.quit_requested();
    let _ = std::fs::remove_dir_all(&dir);
    if let Some(e) = err {
        panic!(
            "{e}\n\nFix docs/spark.d.luau (and docs/SPARK_API.md) or the engine so they agree, then run `SPARK_BLESS=1 cargo test -p spark-api`."
        );
    }
    assert!(quit, "the drift script did not finish");
}

#[test]
fn test_table_matches_the_registry() {
    use spark_script::TaskStatus;
    let api = spark_api::api();
    let dir = std::env::temp_dir().join(format!("spark-api-drift-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("main.luau"), "function start() end").unwrap();
    // The test script is the drift script with the body in start replaced by a plain run.
    let body = script(&api, true).replace("function start()", "local function run()") + "\nrun()\n";
    let test = dir.join("drift.luau");
    std::fs::write(&test, body).unwrap();
    let mut game = ScriptGame::new(dir.join("main.luau")).hot_reload(false).test(test, "tests/drift.luau");
    let mut world = World::new();
    game.start(&mut world);
    for _ in 0..30 {
        run_frame(&mut game, &mut world, 1.0 / 60.0);
        if game.error().is_some() || matches!(game.test_status(), Some(TaskStatus::Done | TaskStatus::Failed)) {
            break;
        }
    }
    let err = game.error().map(str::to_string);
    let status = game.test_status();
    let _ = std::fs::remove_dir_all(&dir);
    if let Some(e) = err {
        panic!("{e}");
    }
    assert!(matches!(status, Some(TaskStatus::Done)), "test script did not finish: {status:?}");
}

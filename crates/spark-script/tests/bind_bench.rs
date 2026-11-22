//! Micro benchmark of the hot Object field accessors (C-7): read and write `obj.position` (and the other generated
//! fields) on 10k objects per frame.
//!
//! Run it in release mode, it prints the best of several runs:
//! `cargo test -p spark-script --release --test bind_bench -- --ignored --nocapture`

use std::time::{Duration, Instant};

use spark_core::{Game, World, run_frame};
use spark_script::ScriptGame;

const OBJECTS: usize = 10_000;
const FRAMES: usize = 40;
const RUNS: usize = 5;

/// Best (lowest) time per frame of `body`, run for every object, over `RUNS` runs of `FRAMES` frames.
fn bench(body: &str) -> Duration {
    bench_with("", body)
}

/// Like [`bench`], with extra spawn properties (`", body = \"static\""`) for objects that need a component.
fn bench_with(props: &str, body: &str) -> Duration {
    let src = format!(
        "local objs = {{}}\n\
         function start()\n  for i = 1, {OBJECTS} do objs[i] = spawn(Mesh.cube(), \"red\", {{ position = vec3(i, 0, 0){props} }}) end\nend\n\
         function update(dt)\n  for i = 1, #objs do\n    local o = objs[i]\n    {body}\n  end\nend\n"
    );
    static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let dir =
        std::env::temp_dir().join(format!("spark-bench-{}-{}", std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("main.luau");
    std::fs::write(&path, &src).unwrap();
    let mut best = Duration::MAX;
    for _ in 0..RUNS {
        let mut game = ScriptGame::new(path.clone()).hot_reload(false).timeout(None);
        let mut world = World::new();
        world.physics.set_backend(Box::new(spark_physics::RapierBackend::new()));
        game.start(&mut world);
        run_frame(&mut game, &mut world, 1.0 / 60.0); // spawns
        run_frame(&mut game, &mut world, 1.0 / 60.0); // warm up
        let t = Instant::now();
        for _ in 0..FRAMES {
            run_frame(&mut game, &mut world, 1.0 / 60.0);
        }
        let per_frame = t.elapsed() / FRAMES as u32;
        assert!(game.error().is_none(), "{:?}", game.error());
        best = best.min(per_frame);
    }
    let _ = std::fs::remove_dir_all(&dir);
    best
}

#[test]
#[ignore = "benchmark: run in release mode with --ignored --nocapture"]
fn object_field_access_per_frame() {
    let cases = [
        ("position read+write", "local p = o.position\n    o.position = p"),
        ("position read+write (new vec)", "local p = o.position\n    o.position = vec3(p.x + 0.001, p.y, p.z)"),
        ("rotation read+write", "local r = o.rotation\n    o.rotation = r"),
        ("scale read+write", "local s = o.scale\n    o.scale = s"),
        ("name read+write", "local n = o.name\n    o.name = n"),
        ("visible read+write", "local v = o.visible\n    o.visible = v"),
        ("x read+write", "o.x = o.x + 0.001"),
        ("color read+write", "local c = o.color\n    o.color = c"),
        ("metallic read+write", "local m = o.metallic\n    o.metallic = m"),
        ("blend read+write", "local b = o.blend\n    o.blend = b"),
        ("double_sided read+write", "local d = o.double_sided\n    o.double_sided = d"),
    ];
    for (label, body) in cases {
        let t = bench(body);
        println!("BENCH {label:<32} {:>9.3} ms/frame ({OBJECTS} objects)", t.as_secs_f64() * 1000.0);
    }
}

/// Body scalars of C-9: `velocity` read+write on bodies (static bodies, so the physics step stays out of the numbers).
#[test]
#[ignore = "benchmark: run in release mode with --ignored --nocapture"]
fn body_field_access_per_frame() {
    let cases = [
        ("control: x on static bodies", "o.x = o.x + 0.001"),
        ("body velocity read+write", "local v = o.velocity\n    o.velocity = v"),
        ("body mass read+write", "local m = o.mass\n    o.mass = m"),
        ("body sensor read+write", "local s = o.sensor\n    o.sensor = s"),
    ];
    for (label, body) in cases {
        let t = bench_with(", body = \"static\"", body);
        println!("BENCH {label:<32} {:>9.3} ms/frame ({OBJECTS} objects)", t.as_secs_f64() * 1000.0);
    }
}

/// Light fields of C-9: `obj.light.intensity` read+write.
#[test]
#[ignore = "benchmark: run in release mode with --ignored --nocapture"]
fn light_field_access_per_frame() {
    let t = bench_with(", light = \"point\"", "local l = o.light\n    l.intensity = l.intensity");
    println!("BENCH {:<32} {:>9.3} ms/frame ({OBJECTS} objects)", "light intensity read+write", t.as_secs_f64() * 1000.0);
}

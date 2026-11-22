//! `ScriptGame::seed`: `math.random` is reproducible (finding B6).

use spark_core::{Game, World, run_frame};
use spark_script::ScriptGame;

static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Draws some numbers and stores them in an object's name, where the test can read them.
const SCRIPT: &str = r#"
local out = {}
for i = 1, 8 do out[#out + 1] = tostring(math.random(1, 1000000)) end
out[#out + 1] = tostring(math.random())
function start()
    spawn(Mesh.cube(), "orange", { name = table.concat(out, ",") })
end
"#;

/// Runs the script (and restarts it `restarts` times); returns the drawn numbers after each (re)start.
fn draw(seed: Option<u64>, restarts: usize) -> Vec<String> {
    let dir =
        std::env::temp_dir().join(format!("spark-seed-{}-{}", std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("main.luau");
    std::fs::write(&path, SCRIPT).unwrap();
    let mut game = ScriptGame::new(path).hot_reload(false).seed(seed);
    let mut world = World::new();
    world.physics.set_backend(Box::new(spark_physics::RapierBackend::new()));
    game.start(&mut world);
    for _ in 0..2 {
        run_frame(&mut game, &mut world, 1.0 / 60.0);
    }
    assert!(game.error().is_none(), "{:?}", game.error());
    let mut seqs = Vec::new();
    for i in 0..=restarts {
        if i > 0 {
            world.request_restart();
            for _ in 0..2 {
                run_frame(&mut game, &mut world, 1.0 / 60.0);
            }
            assert!(game.error().is_none(), "{:?}", game.error());
        }
        seqs.push(world.scene.iter().map(|(_, o)| o.name.clone()).collect::<Vec<_>>().join("|"));
    }
    drop(game);
    let _ = std::fs::remove_dir_all(&dir);
    seqs
}

#[test]
fn same_seed_same_sequence_different_seed_different() {
    let a = draw(Some(7), 0);
    let b = draw(Some(7), 0);
    let c = draw(Some(8), 0);
    assert!(!a[0].is_empty() && a[0].contains(','), "{a:?}");
    assert_eq!(a, b);
    assert_ne!(a, c);
}

#[test]
fn restart_repeats_the_sequence() {
    let s = draw(Some(42), 1);
    assert_eq!(s[0], s[1], "a restart must reseed");
}

#[test]
fn seeds_above_32_bits_still_differ() {
    assert_ne!(draw(Some(1), 0), draw(Some((1 << 40) + 5), 0));
}

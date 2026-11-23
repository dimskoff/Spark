//! Every mesh made by the Luau API remembers its recipe (`MeshSource`), and the recipe rebuilds the same vertices.

mod common;

use spark_core::{Game, MeshId, MeshSource, Vec3, World};
use spark_script::ScriptGame;

fn run(dir_name: &str, code: &str, with_rig: bool) -> (World, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("spark-{dir_name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("main.luau"), code).unwrap();
    if with_rig {
        common::write_test_rig(&dir.join("assets/rig.glb"));
    }
    let mut game = ScriptGame::new(dir.join("main.luau")).hot_reload(false);
    let mut world = World::new();
    game.start(&mut world);
    if let Some(e) = game.error() {
        panic!("script failed:\n{e}");
    }
    (world, dir)
}

#[test]
fn primitives_record_recipes_and_rebuild_identically() {
    let code = r#"
        Mesh.cube(2)
        Mesh.box(1, 2, 3)
        Mesh.box(1, 2, 3, { tile = 0.5 })
        Mesh.box(2, 2, 2, {})
        Mesh.plane(4, 6)
        Mesh.plane(4, 6, { cell = 1 })
        Mesh.quad(2, 3)
        Mesh.quad(2, 3, { tile = 1, cell = 1 })
        Mesh.sphere(0.75, 12)
        Mesh.cylinder(0.5, 2, 8)
        Mesh.triangles({ vec3(0,0,0), vec3(1,0,0), vec3(0,1,0) })
        Mesh.custom({ vec3(0,0,0), vec3(1,0,0), vec3(0,1,0) }, { 1, 2, 3 })
    "#;
    let (world, dir) = run("mesh-recipes", code, false);
    let a = &world.assets;
    assert_eq!(a.mesh_count(), 12);
    assert_eq!(*a.mesh_source(MeshId(1)), MeshSource::Box { size: Vec3::new(1.0, 2.0, 3.0), tile: None, cell: None, surface: false });
    assert!(matches!(a.mesh_source(MeshId(2)), MeshSource::Box { tile: Some(t), surface: true, .. } if *t == 0.5));
    assert!(matches!(a.mesh_source(MeshId(3)), MeshSource::Box { tile: None, cell: None, surface: true, .. }));
    assert_eq!(*a.mesh_source(MeshId(11)), MeshSource::RuntimeOnly);
    for i in 0..11 {
        let id = MeshId(i);
        let rebuilt = a.mesh_source(id).build_primitive().unwrap_or_else(|| panic!("mesh {i} has no primitive recipe"));
        let original = a.mesh(id).unwrap();
        assert_eq!(rebuilt.vertices, original.vertices, "mesh {i} vertices");
        assert_eq!(rebuilt.indices, original.indices, "mesh {i} indices");
        assert_eq!(rebuilt.shape, original.shape, "mesh {i} shape");
    }
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn loaded_model_meshes_record_path_and_node() {
    let (mut world, dir) = run("mesh-recipes-model", "Model.load(\"assets/rig.glb\")", true);
    let mut found = 0;
    for i in 0..world.assets.mesh_count() as u32 {
        let id = MeshId(i);
        let source = world.assets.mesh_source(id).clone();
        let MeshSource::Asset { asset, node, .. } = &source else { continue };
        assert_eq!(asset, "assets/rig.glb");
        assert!(!node.is_empty());
        assert_eq!(source.resolve(&mut world.assets).unwrap(), id);
        assert_eq!(source.build(&mut world.assets).unwrap(), *world.assets.mesh(id).unwrap());
        found += 1;
    }
    assert!(found > 0, "no mesh recorded an asset recipe");
    let _ = std::fs::remove_dir_all(dir);
}

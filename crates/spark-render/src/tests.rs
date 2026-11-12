//! Renderer tests (headless GPU).

use crate::consts::*;
use crate::gpu_util::*;
use crate::lighting::*;
use crate::uniforms::*;
use crate::*;
use spark_core::*;
use spark_core::glam::Quat;
use spark_core::{Material, MeshData};

#[test]
fn uniforms_are_16_byte_aligned() {
    assert_eq!(size_of::<FrameUniform>() % 16, 0);
    assert_eq!(size_of::<ObjectUniform>() % 16, 0);
    // 64 objects per draw must fit the 16 KB uniform binding every backend (incl. GL) offers.
    assert!(size_of::<ObjectUniform>() * MAX_INSTANCES <= 16 * 1024);
}

#[test]
fn cube_face_mask_covers_every_face_a_sphere_touches() {
    // Every point of the sphere lands in the face of its major axis: that face must be in the mask.
    let face_of = |v: Vec3| {
        let a = v.abs();
        if a.x >= a.y && a.x >= a.z {
            if v.x > 0.0 { 0 } else { 1 }
        } else if a.y >= a.z {
            if v.y > 0.0 { 2 } else { 3 }
        } else if v.z > 0.0 {
            4
        } else {
            5
        }
    };
    let mut seed = 7u32;
    let mut rnd = || {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (seed >> 8) as f32 / (1u32 << 24) as f32 * 2.0 - 1.0
    };
    for _ in 0..2000 {
        let d = Vec3::new(rnd(), rnd(), rnd()) * 6.0;
        let r = rnd().abs() * 2.0;
        let mask = cube_face_mask(d, r);
        for _ in 0..64 {
            let p = d + Vec3::new(rnd(), rnd(), rnd()).normalize_or_zero() * r;
            assert!(mask & (1 << face_of(p)) != 0, "d {d} r {r} mask {mask:06b} misses {p}");
        }
    }
    // A small box well inside +X reaches only that face.
    assert_eq!(cube_face_mask(Vec3::new(4.0, 0.2, -0.3), 0.5), 1);
    // A sphere around the light reaches all of them.
    assert_eq!(cube_face_mask(Vec3::new(0.1, 0.0, 0.0), 1.0), 0b11_1111);
}

#[test]
fn normal_matrix_keeps_normals_perpendicular_under_shear() {
    // Child rotated 45° under a parent scaled (3, 1, 1): the world matrix has shear.
    let parent = Mat4::from_scale(Vec3::new(3.0, 1.0, 1.0));
    let child = Mat4::from_quat(Quat::from_rotation_z(std::f32::consts::FRAC_PI_4));
    let model = parent * child;
    let nm = normal_matrix(&model);
    // A surface with local normal +Y contains the local +X direction.
    let tangent = model.transform_vector3(Vec3::X);
    let normal = nm.transform_vector3(Vec3::Y).normalize();
    assert!(tangent.dot(normal).abs() < 1e-5, "normal not perpendicular: {}", tangent.dot(normal));
}

#[test]
fn normal_matrix_degenerate_is_identity() {
    assert_eq!(normal_matrix(&Mat4::from_scale(Vec3::ZERO)), Mat4::IDENTITY);
}

/// Real GPU round trip: a white unlit cube in front of the camera ends up in the image.
/// Skipped when the machine has no GPU / software Vulkan (CI installs lavapipe).
#[test]
fn headless_render_draws_scene() {
    let mut r = match Renderer::new_headless(64, 64) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("skipping GPU test: {e}");
            return;
        }
    };
    let mut world = World::default();
    world.scene.background = Color::BLACK;
    let cube = world.add_mesh(MeshData::cube(2.0));
    world.spawn(cube, Material::unlit(Color::WHITE));
    world.scene.camera.position = Vec3::new(0.0, 0.0, 4.0);
    world.scene.camera.look_at(Vec3::ZERO);
    let image = r.screenshot(&world).expect("screenshot");
    assert_eq!((image.width, image.height), (64, 64));
    let center = image.pixel(32, 32);
    let corner = image.pixel(0, 0);
    assert!(center[0] > 200, "cube not drawn: center = {center:?}");
    assert!(corner[0] < 30, "background not cleared: corner = {corner:?}");
    assert!(r.last_stats().draw_calls >= 1);
    assert!(!r.is_device_lost());
}

fn test_world() -> World {
    let mut world = World::default();
    world.scene.background = Color::BLACK;
    let cube = world.add_mesh(MeshData::cube(2.0));
    // 4x brighter than white: only an HDR scene keeps that until tonemapping.
    world.spawn(cube, Material::unlit(Color::rgb(4.0, 4.0, 4.0)));
    world.scene.camera.position = Vec3::new(0.0, 0.0, 4.0);
    world.scene.camera.look_at(Vec3::ZERO);
    world
}

/// All ten built-in effects together render without GPU validation errors.
#[test]
fn builtin_effects_render() {
    use spark_core::render::*;
    let Ok(mut r) = Renderer::new_headless(96, 64) else { return eprintln!("skipping GPU test: no adapter") };
    let mut world = test_world();
    world.render.postfx = PostProcessing {
        tonemap: Tonemap::Aces,
        bloom: Some(Bloom::default()),
        ssao: Some(Ssao::default()),
        motion_blur: Some(MotionBlur::default()),
        dof: Some(DepthOfField::default()),
        color_grading: Some(ColorGrading::default()),
        fxaa: true,
        chromatic_aberration: Some(ChromaticAberration::default()),
        film_grain: Some(FilmGrain::default()),
        vignette: Some(Vignette::default()),
    };
    let scope = r.device.push_error_scope(wgpu::ErrorFilter::Validation);
    for frame in 1..=3 {
        world.time.frame = frame;
        world.scene.camera.position.x = frame as f32 * 0.1; // moving camera: motion blur path
        r.screenshot(&world).expect("screenshot");
    }
    let error = pollster::block_on(scope.pop());
    assert!(error.is_none(), "GPU validation error: {error:?}");
    assert_eq!(r.last_stats().post_passes, 10);
}

/// PBR (metal + rough dielectric), normal maps, emissive and mesh LOD render without GPU
/// validation errors, with and without MSAA.
#[test]
fn pbr_normal_map_and_lod_render() {
    use spark_core::ImageData;
    let Ok(mut r) = Renderer::new_headless(64, 64) else { return eprintln!("skipping GPU test: no adapter") };
    let mut world = World::default();
    world.scene.background = Color::BLACK;
    let normal = world.assets.add_texture(ImageData::checker(16, 4, Color::rgb(0.5, 0.5, 1.0), Color::rgb(0.7, 0.5, 0.8)));
    let base = world.assets.add_texture(ImageData::checker(16, 2, Color::WHITE, Color::GRAY));
    let sphere = world.add_mesh(MeshData::sphere(0.8, 16));
    let cube = world.add_mesh(MeshData::cube(1.0));
    let metal = Material::pbr(Color::WHITE, 1.0, 0.2).with_normal_map(Some(normal));
    let a = world.spawn(sphere, metal);
    world.scene.get_mut(a).unwrap().position = Vec3::new(-1.0, 0.0, 0.0);
    let mut plastic = Material::pbr(Color::RED, 0.0, 0.8).with_normal_map(Some(normal));
    plastic.texture = Some(base);
    let b = world.spawn(cube, plastic);
    world.scene.get_mut(b).unwrap().position = Vec3::new(1.0, 0.0, 0.0);
    let classic = Material::color(Color::GREEN).with_normal_map(Some(normal));
    let c = world.spawn(cube, classic);
    world.scene.get_mut(c).unwrap().position = Vec3::new(0.0, 1.0, 0.0);
    world.scene.camera.position = Vec3::new(0.0, 0.0, 5.0);
    world.scene.camera.look_at(Vec3::ZERO);
    let scope = r.device.push_error_scope(wgpu::ErrorFilter::Validation);
    for samples in [4, 1] {
        world.render.msaa = samples;
        r.screenshot(&world).expect("screenshot");
    }
    let error = pollster::block_on(scope.pop());
    assert!(error.is_none(), "GPU validation error: {error:?}");
}

/// A shadowed point light: a blocker below it darkens the floor (-Y face of the cube), one beside it darkens the
/// wall (+X face). Each face only draws the casters that reach it, so this also checks that culling keeps them.
#[test]
fn light_shadow_cube_faces_keep_their_casters() {
    use spark_core::Light;
    let Ok(mut r) = Renderer::new_headless(64, 64) else { return eprintln!("skipping GPU test: no adapter") };
    let mut world = World::default();
    world.scene.background = Color::BLACK;
    world.scene.ambient = Color::BLACK;
    world.scene.sun.intensity = 0.0;
    world.render.shadows = false;
    let cube = world.add_mesh(MeshData::cube(1.0));
    let white = Material::color(Color::WHITE);
    let floor = world.spawn(cube, white);
    let o = world.scene.get_mut(floor).unwrap();
    o.position = Vec3::new(0.0, -0.05, 0.0);
    o.scale = Vec3::new(12.0, 0.1, 12.0);
    let wall = world.spawn(cube, white);
    let o = world.scene.get_mut(wall).unwrap();
    o.position = Vec3::new(3.05, 2.0, 0.0);
    o.scale = Vec3::new(0.1, 6.0, 12.0);
    let lamp = world.spawn_empty();
    let o = world.scene.get_mut(lamp).unwrap();
    o.position = Vec3::new(0.0, 2.0, 0.0);
    let mut light = Light::point(Color::WHITE, 3.0, 9.0);
    light.shadows = true;
    o.light = Some(light);
    let mut shade = |world: &mut World, blocker: Vec3, eye: Vec3, at: Vec3| {
        world.scene.camera.position = eye;
        world.scene.camera.look_at(at);
        let lit = r.screenshot(world).expect("screenshot").pixel(32, 32);
        let b = world.spawn(cube, white);
        let o = world.scene.get_mut(b).unwrap();
        o.position = blocker;
        o.scale = Vec3::splat(0.3);
        let dark = r.screenshot(world).expect("screenshot").pixel(32, 32);
        world.scene.destroy(b);
        (lit[0], dark[0])
    };
    let (lit, dark) = shade(&mut world, Vec3::new(0.4, 1.0, 0.0), Vec3::new(0.8, 6.0, 3.0), Vec3::new(0.8, 0.0, 0.0));
    assert!(lit > 40 && dark < lit / 3, "floor under the blocker: lit {lit}, shadowed {dark}");
    let (lit, dark) = shade(&mut world, Vec3::new(1.5, 2.0, 0.0), Vec3::new(0.5, 2.0, 3.0), Vec3::new(3.0, 2.0, 0.0));
    assert!(lit > 40 && dark < lit / 3, "wall beside the blocker: lit {lit}, shadowed {dark}");
}

/// Emissive light shows on a black surface in the dark; past its LOD distance an object
/// draws its LOD mesh instead (here a small cube that leaves the edge pixels empty).
#[test]
fn emissive_and_lod_switch() {
    let Ok(mut r) = Renderer::new_headless(64, 64) else { return eprintln!("skipping GPU test: no adapter") };
    let mut world = World::default();
    world.scene.background = Color::BLACK;
    world.scene.ambient = Color::BLACK;
    world.scene.sun.intensity = 0.0;
    let big = world.add_mesh(MeshData::cube(2.0));
    let tiny = world.add_mesh(MeshData::cube(0.5));
    let id = world.spawn(big, Material::pbr(Color::BLACK, 0.0, 1.0).with_emissive(Color::rgb(1.0, 0.0, 0.0)));
    world.scene.camera.position = Vec3::new(0.0, 0.0, 4.0);
    world.scene.camera.look_at(Vec3::ZERO);
    let image = r.screenshot(&world).expect("screenshot");
    let (center, edge) = (image.pixel(32, 32), image.pixel(20, 32));
    assert!(center[0] > 200 && center[1] < 40, "emissive not drawn: {center:?}");
    assert!(edge[0] > 200, "full mesh covers the edge: {edge:?}");
    world.scene.get_mut(id).unwrap().lod = vec![(3.0, tiny)];
    let image = r.screenshot(&world).expect("screenshot");
    assert!(image.pixel(32, 32)[0] > 200, "LOD mesh drawn");
    assert!(image.pixel(20, 32)[0] < 30, "LOD mesh replaces the full mesh: {:?}", image.pixel(20, 32));
}

/// Gradient sky: up shows `top`, down shows `bottom`; with a cubemap the face in view shows.
#[test]
fn sky_gradient_and_cubemap() {
    use spark_core::{ImageData, Sky};
    let Ok(mut r) = Renderer::new_headless(32, 32) else { return eprintln!("skipping GPU test: no adapter") };
    let mut world = World::default();
    world.scene.background = Color::BLACK;
    world.scene.sky = Some(Sky::gradient(Color::rgb(0.0, 0.0, 1.0), Color::rgb(0.0, 1.0, 0.0)));
    world.scene.camera.position = Vec3::ZERO;
    world.scene.camera.look_at(Vec3::new(0.0, 10.0, -0.01)); // straight up
    let up = r.screenshot(&world).expect("screenshot").pixel(16, 16);
    world.scene.camera.look_at(Vec3::new(0.0, -10.0, -0.01)); // straight down
    let down = r.screenshot(&world).expect("screenshot").pixel(16, 16);
    assert!(up[2] > 200 && up[1] < 60, "up = {up:?}");
    assert!(down[1] > 200 && down[2] < 60, "down = {down:?}");

    // Faces: +X red, -X black, +Y black, -Y black, +Z black, -Z white (the camera looks along -Z).
    let colors = [Color::rgb(1.0, 0.0, 0.0), Color::BLACK, Color::BLACK, Color::BLACK, Color::BLACK, Color::WHITE];
    let cube = world.assets.add_cubemap(colors.map(|c| ImageData::new(4, 4, c.to_rgba8().repeat(16)))).unwrap();
    world.scene.sky = Some(Sky::cubemap(cube));
    let scope = r.device.push_error_scope(wgpu::ErrorFilter::Validation);
    for samples in [4, 1] {
        world.render.msaa = samples;
        world.scene.camera.look_at(Vec3::new(0.0, 0.0, -10.0));
        let fwd = r.screenshot(&world).expect("screenshot").pixel(16, 16);
        assert!(fwd.iter().take(3).all(|&c| c > 200), "-Z face: {fwd:?}");
        world.scene.camera.look_at(Vec3::new(10.0, 0.0, 0.0));
        let right = r.screenshot(&world).expect("screenshot").pixel(16, 16);
        assert!(right[0] > 200 && right[1] < 60, "+X face: {right:?}");
    }
    let error = pollster::block_on(scope.pop());
    assert!(error.is_none(), "GPU validation error: {error:?}");
}

/// Sky lighting: in a scene with no ambient color and no sun, a white diffuse cube is lit by
/// the sky only when `lighting` is above 0, and a metal reflects the sky.
#[test]
fn sky_lights_the_scene() {
    use spark_core::Sky;
    let Ok(mut r) = Renderer::new_headless(32, 32) else { return eprintln!("skipping GPU test: no adapter") };
    let mut world = World::default();
    world.scene.ambient = Color::BLACK;
    world.scene.sun.intensity = 0.0;
    let cube = world.add_mesh(MeshData::cube(2.0));
    let id = world.spawn(cube, Material::color(Color::WHITE));
    world.scene.camera.position = Vec3::new(0.0, 0.0, 4.0);
    world.scene.camera.look_at(Vec3::ZERO);
    let sky = Sky::gradient(Color::rgb(0.8, 0.8, 0.8), Color::rgb(0.8, 0.8, 0.8));
    world.scene.sky = Some(sky);
    let dark = r.screenshot(&world).expect("screenshot").pixel(16, 16);
    assert!(dark[0] < 20, "lighting 0 keeps the flat (black) ambient: {dark:?}");
    world.scene.sky = Some(sky.with_lighting(1.0));
    let lit = r.screenshot(&world).expect("screenshot").pixel(16, 16);
    assert!(lit[0] > 150, "sky lights the cube: {lit:?}");
    world.scene.get_mut(id).unwrap().material = Material::pbr(Color::WHITE, 1.0, 0.1);
    let metal = r.screenshot(&world).expect("screenshot").pixel(16, 16);
    assert!(metal[0] > 150, "metal reflects the sky: {metal:?}");
}

/// Values above 1 reach the tonemapper: Reinhard x / (1 + x) stays above 0.5 (sRGB 188) only
/// if the scene kept x > 1; an 8-bit scene clips to 1.0 first and gives exactly ~188.
#[test]
fn hdr_scene_reaches_tonemapper() {
    use spark_core::render::Tonemap;
    let Ok(mut r) = Renderer::new_headless(64, 64) else { return eprintln!("skipping GPU test: no adapter") };
    if r.scene_format != HDR_FORMAT {
        return eprintln!("skipping: adapter has no HDR render target");
    }
    let mut world = test_world();
    world.render.postfx.tonemap = Tonemap::Reinhard;
    let center = r.screenshot(&world).expect("screenshot").pixel(32, 32);
    assert!((200..=252).contains(&center[0]), "center = {center:?}");
}

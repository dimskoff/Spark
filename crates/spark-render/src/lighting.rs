//! Light collection and shadow maps: sun cascade-less map, point / spot shadow cubes.

use bytemuck::Zeroable;
use spark_core::{
    Mat4, Vec3, Vec4,
};

use crate::consts::*;
use crate::culling::*;
use crate::gpu_util::*;
use crate::renderer::*;
use crate::resources::*;
use crate::uniforms::*;
/// Lights that can touch the view (sphere vs frustum), closest first, packed for the shader.
/// Always sorted, ties broken by object index, so the chosen set does not flicker between frames.
/// Also returns the lights in that list that asked for shadows (`ShadowRequest::light` = their index in it).
pub(crate) fn collect_lights(
    scene: &spark_core::Scene,
    mats: &[Mat4],
    vis: &[bool],
    planes: &[Vec4; 6],
) -> ([LightUniform; MAX_LIGHTS], usize, Vec<ShadowRequest>) {
    let cam = scene.camera.position;
    let mut found: Vec<(f32, spark_core::ObjectId, LightUniform, bool)> = Vec::new();
    for (id, obj) in scene.iter() {
        let Some(light) = obj.light else { continue };
        let i = id.index() as usize;
        if light.intensity <= 0.0 || !vis[i] {
            continue;
        }
        let m = mats[i];
        let pos = m.transform_point3(Vec3::ZERO);
        if sphere_outside(planes, pos, light.range) {
            continue;
        }
        let dir = m.transform_vector3(Vec3::NEG_Z).normalize_or(Vec3::NEG_Z);
        let c = light.color.to_linear();
        let li = light.intensity;
        let (spot, outer, inner) = match light.kind {
            spark_core::LightKind::Point => (0.0, -2.0, -1.0),
            spark_core::LightKind::Spot { angle, softness } => {
                let half = angle * 0.5;
                (1.0, half.cos(), (half * (1.0 - softness.clamp(0.0, 1.0))).cos())
            }
        };
        let u = LightUniform {
            position: pos.extend(light.range).to_array(),
            color: [c[0] * li, c[1] * li, c[2] * li, spot],
            direction: dir.extend(outer).to_array(),
            params: [inner, 0.0, 0.0, 0.0],
        };
        found.push(((pos.distance(cam) - light.range).max(0.0), id, u, light.shadows));
    }
    if found.len() > MAX_LIGHTS {
        found.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.index().cmp(&b.1.index())));
    }
    let mut out = [LightUniform::zeroed(); MAX_LIGHTS];
    let n = found.len().min(MAX_LIGHTS);
    let mut shadows = Vec::new();
    for (i, (slot, (distance, id, u, cast))) in out.iter_mut().zip(found).enumerate() {
        *slot = u;
        if cast {
            let p = u.position;
            shadows.push(ShadowRequest { distance, id, position: Vec3::new(p[0], p[1], p[2]), range: p[3], light: i });
        }
    }
    (out, n, shadows)
}

/// Gives the closest shadow requests a slot (a light keeps the slot it had) and picks which cubes to render:
/// every slot whose light is new, moved or changed range, plus `updates` of the others round robin (0 = all).
/// Returns (slot, position, range) of the cubes to render; writes the slot into each light's `params.y`.
pub(crate) fn assign_light_shadows(
    slots: &mut [Option<LightSlot>; MAX_SHADOWED],
    cursor: &mut usize,
    mut requests: Vec<ShadowRequest>,
    max: usize,
    updates: usize,
    lights: &mut [LightUniform; MAX_LIGHTS],
) -> Vec<(usize, Vec3, f32)> {
    requests.sort_by(|a, b| a.distance.total_cmp(&b.distance).then(a.id.index().cmp(&b.id.index())));
    let max = max.min(MAX_SHADOWED);
    requests.truncate(max);
    for (i, slot) in slots.iter_mut().enumerate() {
        if i >= max || slot.is_some_and(|s| !requests.iter().any(|r| r.id == s.id)) {
            *slot = None;
        }
    }
    for r in &requests {
        let i = match slots.iter().position(|s| s.is_some_and(|s| s.id == r.id)) {
            Some(i) => i,
            None => slots[..max].iter().position(Option::is_none).expect("a free slot: requests <= max"),
        };
        let fresh = slots[i].is_some_and(|s| s.fresh && s.position.distance_squared(r.position) < 1e-6 && (s.range - r.range).abs() < 1e-4);
        slots[i] = Some(LightSlot { id: r.id, position: r.position, range: r.range, fresh });
        lights[r.light].params[1] = (i + 1) as f32;
    }
    let mut render: Vec<usize> = (0..MAX_SHADOWED).filter(|&i| slots[i].is_some_and(|s| !s.fresh)).collect();
    let stale: Vec<usize> = (0..MAX_SHADOWED).filter(|&i| slots[i].is_some_and(|s| s.fresh)).collect();
    if updates == 0 {
        render.extend(stale);
    } else if !stale.is_empty() {
        for k in 0..updates.min(stale.len()) {
            render.push(stale[(*cursor + k) % stale.len()]);
        }
        *cursor = (*cursor + updates) % stale.len();
    }
    render
        .into_iter()
        .map(|i| {
            let s = slots[i].as_mut().expect("assigned slot");
            s.fresh = true;
            (i, s.position, s.range)
        })
        .collect()
}

pub(crate) fn light_shadow_map(device: &wgpu::Device, size: u32, layers: u32) -> LightShadowMap {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("spark light shadow map"),
        size: wgpu::Extent3d { width: size, height: size, depth_or_array_layers: layers },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: SHADOW_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor {
        dimension: Some(wgpu::TextureViewDimension::D2Array),
        ..Default::default()
    });
    let layers = (0..layers)
        .map(|l| {
            texture.create_view(&wgpu::TextureViewDescriptor {
                dimension: Some(wgpu::TextureViewDimension::D2),
                base_array_layer: l,
                array_layer_count: Some(1),
                ..Default::default()
            })
        })
        .collect();
    LightShadowMap { size, _texture: texture, view, layers }
}

/// Faces of a shadow cube (bit = face index of `light_face_view_proj`) that a sphere at `d` from the light, radius `r`,
/// can touch. A face's 90 degree frustum is bounded by the planes `axis = +-other axis`; a sphere is out when it lies
/// fully behind one of them, i.e. its signed distance `(axis - |other|) / sqrt(2)` is below `-r`.
pub(crate) fn cube_face_mask(d: Vec3, r: f32) -> u8 {
    let slack = -r * std::f32::consts::SQRT_2;
    let a = d.abs();
    let reach = |axis: f32, o1: f32, o2: f32| axis - o1 >= slack && axis - o2 >= slack;
    let mut mask = 0;
    for (i, (axis, o1, o2)) in [(d.x, a.y, a.z), (-d.x, a.y, a.z), (d.y, a.x, a.z), (-d.y, a.x, a.z), (d.z, a.x, a.y), (-d.z, a.x, a.y)]
        .into_iter()
        .enumerate()
    {
        if reach(axis, o1, o2) {
            mask |= 1 << i;
        }
    }
    mask
}

/// View-projection of one face of a light's shadow cube (+X, -X, +Y, -Y, +Z, -Z), depth 0 (near) .. 1 (far).
pub(crate) fn light_face_view_proj(position: Vec3, range: f32, face: usize) -> Mat4 {
    const FACES: [(Vec3, Vec3); 6] = [
        (Vec3::X, Vec3::NEG_Y),
        (Vec3::NEG_X, Vec3::NEG_Y),
        (Vec3::Y, Vec3::Z),
        (Vec3::NEG_Y, Vec3::NEG_Z),
        (Vec3::Z, Vec3::NEG_Y),
        (Vec3::NEG_Z, Vec3::NEG_Y),
    ];
    let (dir, up) = FACES[face];
    let proj = spark_core::glam::camera::rh::proj::directx::perspective(std::f32::consts::FRAC_PI_2, 1.0, LIGHT_SHADOW_NEAR, range.max(LIGHT_SHADOW_NEAR * 4.0));
    proj * spark_core::glam::camera::rh::view::look_to_mat4(position, dir, up)
}

pub(crate) fn shadow_map(device: &wgpu::Device, size: u32) -> ShadowMap {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("spark shadow map"),
        size: extent((size, size)),
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: SHADOW_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    ShadowMap { size, view: texture.create_view(&Default::default()), _texture: texture }
}

/// Orthographic sun view around the camera, snapped to shadow texels so edges don't shimmer.
pub(crate) fn sun_view_proj(cam: &spark_core::Camera, sun_dir: Vec3, distance: f32, size: u32) -> Mat4 {
    let dir = sun_dir.normalize_or(Vec3::NEG_Y);
    let r = distance.max(1.0) * 0.5;
    let center = cam.position + cam.forward() * r;
    let up = if dir.y.abs() > 0.99 { Vec3::Z } else { Vec3::Y };
    let view = spark_core::glam::camera::rh::view::look_to_mat4(Vec3::ZERO, dir, up);
    let c = view.transform_point3(center);
    let texel = 2.0 * r / size.max(1) as f32;
    let (cx, cy) = ((c.x / texel).floor() * texel, (c.y / texel).floor() * texel);
    // Casters up to 100 m towards the sun (tall buildings, mountains) still cast into the box.
    let proj = spark_core::glam::camera::rh::proj::directx::orthographic(cx - r, cx + r, cy - r, cy + r, -c.z - r - 100.0, -c.z + r);
    proj * view
}

impl Renderer {
    /// Depth-only passes: the sun shadow map, then six faces per point / spot light cube (each draws that light's casters).
    pub(crate) fn draw_shadow_passes(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        shadows: bool,
        shadow_batches: &[Batch3],
        light_render: &[(usize, Vec3, f32)],
        light_batches: &[Batch3],
        light_owner: &[usize],
        light_faces: &[u8],
    ) {
        // Shadow pass.
        if let Some(map) = self.shadow.as_ref().filter(|_| shadows) {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("spark shadow"),
                color_attachments: &[],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &map.view,
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.shadow_pipeline);
            pass.set_bind_group(0, &self.shadow_bind_group, &[]);
            for b in shadow_batches {
                pass.set_bind_group(1, &self.object_bind_group, &[b.offset]);
                let mesh = self.meshes[b.mesh].as_ref().expect("uploaded mesh"); // candidates filtered to have uploaded meshes
                pass.set_vertex_buffer(0, mesh.vertices.slice(..));
                pass.set_index_buffer(mesh.indices.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..mesh.index_count, 0, 0..b.count);
            }
        }

        // Light shadow cubes: six depth passes per light, each drawing that light's casters.
        if let Some(map) = self.light_shadow.as_ref() {
            for (k, &(slot, _, _)) in light_render.iter().enumerate() {
                for face in 0..6 {
                    let layer = slot * 6 + face;
                    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("spark light shadow"),
                        color_attachments: &[],
                        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                            view: &map.layers[layer],
                            depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
                            stencil_ops: None,
                        }),
                        timestamp_writes: None,
                        occlusion_query_set: None,
                        multiview_mask: None,
                    });
                    pass.set_pipeline(&self.light_shadow_pipeline);
                    pass.set_bind_group(0, &self.light_shadow_bind_group, &[(layer as u64 * self.light_shadow_stride) as u32]);
                    let bit = 1u8 << face;
                    for (b, _) in light_batches.iter().zip(light_owner.iter().zip(light_faces)).filter(|(_, (o, f))| **o == k && **f & bit != 0) {
                        pass.set_bind_group(1, &self.object_bind_group, &[b.offset]);
                        let mesh = self.meshes[b.mesh].as_ref().expect("uploaded mesh"); // candidates filtered to have uploaded meshes
                        pass.set_vertex_buffer(0, mesh.vertices.slice(..));
                        pass.set_index_buffer(mesh.indices.slice(..), wgpu::IndexFormat::Uint32);
                        pass.draw_indexed(0..mesh.index_count, 0, 0..b.count);
                    }
                }
            }
        }
    }
}

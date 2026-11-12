//! The 3D scene pass: culling, batching, shadow passes and the main color pass.

use std::collections::HashMap;

use spark_core::material::BlendMode;
use spark_core::{
    Mat4,
    ShaderKind, TextureFilter, Vec3, World,
};

use crate::consts::*;
use crate::culling::*;
use crate::gpu_util::*;
use crate::lighting::*;
use crate::renderer::*;
use crate::resources::*;
use crate::stats::*;
use crate::uniforms::*;

impl Renderer {
    pub(crate) fn draw_scene(&mut self, encoder: &mut wgpu::CommandEncoder, world: &World) -> FrameStats {
        self.sync_assets(&world.assets);
        self.sync_shaders(&world.assets);
        let rs = &world.render;
        let samples = self.supported_samples(rs.msaa.max(1));
        self.ensure_scene_target(rs.internal_size(self.width, self.height), samples);
        let internal = self.scene_target.as_ref().expect("scene target").size; // ensure_scene_target() was called

        let scene = &world.scene;
        let cam = &scene.camera;
        let aspect = self.width as f32 / self.height as f32;
        let (view, proj) = (cam.view(), cam.render_projection(aspect));
        let view_proj = proj * view;
        let planes = frustum_planes(&view_proj);
        let sun = scene.sun.color.to_linear();
        let si = scene.sun.intensity;
        let (fog_color, fog) = match scene.fog {
            Some(f) => (f.color.to_linear(), [f.near, f.far, 1.0, 0.0]),
            None => ([0.0; 4], [0.0; 4]),
        };

        // World matrices + visibility of every object in one O(n) pass (interpolated bodies).
        let mut mats = std::mem::take(&mut self.world_matrices);
        let mut vis = std::mem::take(&mut self.world_visible);
        scene.compute_world_transforms(world.time.alpha(), &mut mats, &mut vis);
        let (mut lights, light_count, shadow_requests) = collect_lights(scene, &mats, &vis, &planes);

        // Shadow cubes of point / spot lights (light.shadows = true), the closest `light_shadows` of them.
        let light_max = (rs.light_shadows as usize).min(MAX_SHADOWED);
        let light_size = rs.light_shadow_size.clamp(64, self.device.limits().max_texture_dimension_2d.min(2048));
        let want_light_shadows = light_max > 0 && !shadow_requests.is_empty();
        let layers = (light_max * 6) as u32;
        if want_light_shadows && self.light_shadow.as_ref().is_none_or(|m| m.size != light_size || m.layers.len() as u32 != layers) {
            self.light_shadow = Some(light_shadow_map(&self.device, light_size, layers));
            for s in self.light_slots.iter_mut().flatten() {
                s.fresh = false;
            }
        } else if !want_light_shadows {
            self.light_shadow = None;
            self.light_slots = [None; MAX_SHADOWED];
        }
        let light_render = if want_light_shadows {
            assign_light_shadows(
                &mut self.light_slots,
                &mut self.light_shadow_cursor,
                shadow_requests,
                light_max,
                rs.light_shadow_updates as usize,
                &mut lights,
            )
        } else {
            Vec::new()
        };
        let mut light_vps = [[[0.0f32; 4]; 4]; SHADOW_FACES];
        for (i, s) in self.light_slots.iter().enumerate() {
            if let Some(s) = s {
                for face in 0..6 {
                    light_vps[i * 6 + face] = light_face_view_proj(s.position, s.range, face).to_cols_array_2d();
                }
            }
        }

        // Sun shadow map.
        let max_dim = self.device.limits().max_texture_dimension_2d;
        let shadow_size = rs.shadow_size.clamp(256, max_dim.min(8192));
        let shadows = rs.shadows && si > 0.0 && rs.shadow_distance > 0.0;
        if shadows && self.shadow.as_ref().is_none_or(|m| m.size != shadow_size) {
            self.shadow = Some(shadow_map(&self.device, shadow_size));
        } else if !shadows {
            self.shadow = None;
        }
        let sky = scene.sky;
        let sky_cube = sky.and_then(|s| s.cubemap).map(|c| c.0 as usize).filter(|&c| c < self.cubemaps.len());
        let key = (self.shadow.as_ref().map(|m| m.size), sky_cube, self.light_shadow.as_ref().map(|m| m.size * 64 + m.layers.len() as u32));
        if key != self.frame_group_key {
            let shadow_view = self.shadow.as_ref().map_or(&self.shadow_dummy.view, |m| &m.view);
            let sky_view = sky_cube.map_or(&self.sky_dummy.view, |c| &self.cubemaps[c].view);
            let light_shadow_view = self.light_shadow.as_ref().map_or(&self.light_shadow_dummy.view, |m| &m.view);
            self.frame_bind_group = frame_bind_group(
                &self.device,
                &self.frame_layout,
                &self.frame_buffer,
                shadow_view,
                &self.shadow_sampler,
                sky_view,
                &self.sky_sampler,
                light_shadow_view,
            );
            self.frame_group_key = key;
        }
        let light_vp = if shadows { sun_view_proj(cam, scene.sun.direction, rs.shadow_distance, shadow_size) } else { Mat4::IDENTITY };
        let texel_world = rs.shadow_distance / shadow_size as f32;

        let t = &world.time;
        // Only a new frame moves the history (a screenshot of the same frame keeps it).
        if self.camera_history.is_none_or(|(f, _)| f != t.frame) {
            self.prev_view_proj = self.camera_history.map_or(view_proj, |(_, vp)| vp);
            self.camera_history = Some((t.frame, view_proj));
        }
        let frame = FrameUniform {
            view_proj: view_proj.to_cols_array_2d(),
            view: view.to_cols_array_2d(),
            proj: proj.to_cols_array_2d(),
            inv_view_proj: view_proj.inverse().to_cols_array_2d(),
            camera_pos: cam.position.extend(1.0).to_array(),
            camera: [cam.near.max(0.0001), cam.far, cam.fov.clamp(1.0, 179.0).to_radians(), aspect],
            screen: [internal.0 as f32, internal.1 as f32, self.width as f32, self.height as f32],
            time: [t.elapsed as f32, t.dt, t.unscaled_elapsed as f32, t.frame as f32],
            sun_dir: scene.sun.direction.normalize_or(Vec3::NEG_Y).extend(0.0).to_array(),
            sun_color: [sun[0] * si, sun[1] * si, sun[2] * si, 1.0],
            ambient: scene.ambient.to_linear(),
            fog_color,
            fog,
            light_count: [light_count as u32, 0, 0, 0],
            lights,
            shadow_view_proj: light_vp.to_cols_array_2d(),
            shadow: [flag(shadows), 1.0 / shadow_size as f32, texel_world * 1.5, rs.shadow_distance],
            prev_view_proj: self.prev_view_proj.to_cols_array_2d(),
            sky_top: sky.map_or([0.0; 4], |s| s.top.to_linear()),
            sky_bottom: sky.map_or([0.0; 4], |s| s.bottom.to_linear()),
            sky: match sky {
                Some(s) => [
                    1.0,
                    if sky_cube.is_some() && s.blend > 0.0 { s.blend.min(1.0) } else { 0.0 }, // NaN -> 0
                    s.lighting.max(0.0),
                    sky_cube.map_or(1, |c| self.cubemaps[c].mips) as f32,
                ],
                None => [0.0; 4],
            },
            light_shadow_view_proj: light_vps,
            light_shadow: [flag(self.light_shadow.is_some()), 1.0 / light_size as f32, flag(rs.light_shadow_soft), 0.0],
        };
        self.queue.write_buffer(&self.frame_buffer, 0, bytemuck::bytes_of(&frame));
        if !light_render.is_empty() {
            let stride = self.light_shadow_stride as usize;
            let mut views = vec![0u8; stride * SHADOW_FACES];
            for (i, vp) in light_vps.iter().enumerate() {
                views[i * stride..i * stride + 64].copy_from_slice(bytemuck::bytes_of(vp));
            }
            self.queue.write_buffer(&self.light_shadow_buffer, 0, &views);
        }
        if shadows {
            self.queue.write_buffer(&self.shadow_buffer, 0, bytemuck::bytes_of(&light_vp.to_cols_array_2d()));
        }

        // Candidates: everything visible in the camera or casting into the shadow map.
        let mut cands = std::mem::take(&mut self.candidates);
        cands.clear();
        let mut stats = FrameStats { internal_size: internal, ..Default::default() };
        let fallback = self.asset_shader(rs.shader, ShaderKind::Surface).unwrap_or(ShaderRef::Default);
        for (id, obj) in scene.iter() {
            if obj.mesh.is_none() {
                continue;
            }
            let i = id.index() as usize;
            if !vis[i] {
                continue;
            }
            let model = mats[i];
            let m = &obj.material;
            let bounds = obj.local_bounds();
            let center = model.transform_point3(if bounds.is_empty() { Vec3::ZERO } else { (bounds.min + bounds.max) * 0.5 });
            // Level of detail by camera distance; a missing / empty LOD mesh falls back to the base mesh.
            let uploaded = |m: Option<spark_core::MeshId>| m.map(|m| m.0 as usize).filter(|&m| matches!(self.meshes.get(m), Some(Some(_))));
            let Some(mesh) = uploaded(obj.mesh_at(center.distance(cam.position))).or_else(|| uploaded(obj.mesh)) else { continue };
            let Some(Some(gpu_mesh)) = self.meshes.get(mesh) else { continue };
            let in_view = !outside_frustum(&(view_proj * model), &bounds);
            let transparent = m.is_transparent();
            let caster = shadows && m.cast_shadow && !transparent && !outside_frustum(&(light_vp * model), &bounds);
            let scale = model.x_axis.truncate().length().max(model.y_axis.truncate().length()).max(model.z_axis.truncate().length());
            let radius = if bounds.is_empty() { 0.0 } else { (bounds.max - bounds.min).length() * 0.5 * scale };
            let light_caster = m.cast_shadow
                && !transparent
                && light_render.iter().any(|&(_, p, r)| center.distance_squared(p) < (r + radius) * (r + radius));
            if !in_view {
                stats.culled += 1;
                if !caster && !light_caster {
                    continue;
                }
            }
            let variant = match m.effective_blend() {
                BlendMode::Alpha => VARIANT_ALPHA,
                BlendMode::Additive => VARIANT_ADDITIVE,
                _ => 0,
            } | if m.double_sided { VARIANT_DOUBLE_SIDED } else { 0 };
            let normal_map = m.normal_map.map(|t| t.0 as usize).filter(|&t| t < self.textures.len());
            let orm_map = m.orm_map.map(|t| t.0 as usize).filter(|&t| t < self.textures.len());
            let emissive = m.emissive.to_linear();
            cands.push(Candidate {
                id,
                uniform: ObjectUniform {
                    model: model.to_cols_array_2d(),
                    normal_matrix: normal_matrix(&model).to_cols_array_2d(),
                    color: m.color.to_linear(),
                    data: m.data,
                    info: [m.tiling.x, m.tiling.y, flag(m.unlit), flag(m.pbr)],
                    pbr: [m.metallic, m.roughness, flag(normal_map.is_some()), flag(orm_map.is_some())],
                    emissive: [emissive[0], emissive[1], emissive[2], 0.0],
                },
                shader: self.asset_shader(m.shader, ShaderKind::Surface).unwrap_or(fallback),
                variant,
                mesh,
                texture: m.texture.map(|t| t.0 as usize).filter(|&t| t < self.textures.len()),
                normal_map,
                orm_map,
                depth: -view.transform_point3(center).z,
                in_view,
                caster,
                light_caster,
                center,
                radius,
                triangles: (gpu_mesh.index_count / 3) as u64,
            });
        }
        self.world_matrices = mats;
        self.world_visible = vis;

        // Pipelines for every (shader, variant) in use; broken shaders fall back to the default.
        let mut usable: HashMap<(ShaderRef, u8), bool> = HashMap::new();
        for c in cands.iter_mut().filter(|c| c.in_view) {
            let key = (c.shader, c.variant);
            let ok = match usable.get(&key) {
                Some(&ok) => ok,
                None => {
                    let ok = self.ensure_shader_pipeline(c.shader, (self.scene_format, c.variant, samples));
                    usable.insert(key, ok);
                    ok
                }
            };
            if !ok {
                c.shader = ShaderRef::Default;
                self.ensure_shader_pipeline(ShaderRef::Default, (self.scene_format, c.variant, samples));
            }
        }

        // Order: opaque grouped by state (instanced), then transparent back to front.
        let mut order = std::mem::take(&mut self.order);
        order.clear();
        order.extend((0..cands.len() as u32).filter(|&i| cands[i as usize].in_view));
        let transparent = |c: &Candidate| c.variant & (VARIANT_ALPHA | VARIANT_ADDITIVE) != 0;
        order.sort_by(|&a, &b| {
            let (a, b) = (&cands[a as usize], &cands[b as usize]);
            match (transparent(a), transparent(b)) {
                (false, false) => (a.variant, a.shader, a.texture, a.normal_map, a.orm_map, a.mesh).cmp(&(b.variant, b.shader, b.texture, b.normal_map, b.orm_map, b.mesh)),
                (true, true) => b.depth.total_cmp(&a.depth),
                (ta, tb) => ta.cmp(&tb),
            }
        });
        let scene_count = order.len();
        order.extend((0..cands.len() as u32).filter(|&i| cands[i as usize].caster));
        order[scene_count..].sort_by_key(|&i| cands[i as usize].mesh);
        let sun_end = order.len();
        // One run of casters per light cube to render (objects whose bounding sphere reaches the light). The object
        // carrying the light (a bulb mesh around it) would black the whole cube out, so it never casts for its own.
        let mut light_ends = Vec::with_capacity(light_render.len());
        for &(slot, p, r) in &light_render {
            let owner = self.light_slots[slot].map(|s| s.id);
            let start = order.len();
            order.extend((0..cands.len() as u32).filter(|&i| {
                let c = &cands[i as usize];
                c.light_caster && Some(c.id) != owner && c.center.distance_squared(p) < (r + c.radius) * (r + c.radius)
            }));
            order[start..].sort_by_key(|&i| cands[i as usize].mesh);
            light_ends.push(order.len());
        }

        let mut scratch = std::mem::take(&mut self.scratch);
        scratch.clear();
        let mut batches = std::mem::take(&mut self.batches);
        let mut shadow_batches = std::mem::take(&mut self.shadow_batches);
        let mut light_batches = std::mem::take(&mut self.light_batches);
        let mut light_owner = std::mem::take(&mut self.light_batch_owner);
        let mut light_faces = std::mem::take(&mut self.light_batch_faces);
        batches.clear();
        shadow_batches.clear();
        light_batches.clear();
        light_owner.clear();
        light_faces.clear();
        let align = self.object_align as usize;
        let mut owner = 0;
        for (slot, &ci) in order.iter().enumerate() {
            let c = &cands[ci as usize];
            let shadow_part = slot >= scene_count;
            let light_part = slot >= sun_end;
            while light_part && slot >= light_ends[owner] {
                owner += 1;
            }
            if !shadow_part {
                stats.triangles += c.triangles;
            }
            let list = if light_part {
                &mut light_batches
            } else if shadow_part {
                &mut shadow_batches
            } else {
                &mut batches
            };
            let same = list.last().is_some_and(|b: &Batch3| {
                (b.count as usize) < MAX_INSTANCES
                    && b.mesh == c.mesh
                    && (!light_part || light_owner.last() == Some(&owner))
                    && (shadow_part
                        || (b.shader == c.shader && b.variant == c.variant && b.texture == c.texture && b.normal_map == c.normal_map && b.orm_map == c.orm_map))
            });
            let faces = if light_part { cube_face_mask(c.center - light_render[owner].1, c.radius) } else { 0 };
            if same {
                list.last_mut().expect("batch").count += 1; // checked is_some() above
                if light_part {
                    *light_faces.last_mut().expect("light batch") |= faces;
                }
            } else {
                if light_part {
                    light_owner.push(owner);
                    light_faces.push(faces);
                }
                scratch.resize(scratch.len().div_ceil(align) * align, 0);
                list.push(Batch3 {
                    shader: c.shader,
                    variant: c.variant,
                    mesh: c.mesh,
                    texture: c.texture,
                    normal_map: c.normal_map,
                    orm_map: c.orm_map,
                    offset: scratch.len() as u32,
                    count: 1,
                });
            }
            scratch.extend_from_slice(bytemuck::bytes_of(&c.uniform));
        }
        stats.draw_calls = batches.len() as u32;
        self.candidates = cands;
        self.order = order;

        // The last batch binds a full 64-object window: keep that much room after it.
        let needed = scratch.len() + MAX_INSTANCES * size_of::<ObjectUniform>();
        if needed > self.object_capacity {
            self.object_capacity = needed.next_power_of_two();
            let (buffer, bind_group) = object_buffer(&self.device, &self.object_layout, self.object_capacity);
            self.object_buffer = buffer;
            self.object_bind_group = bind_group;
        }
        if !scratch.is_empty() {
            self.queue.write_buffer(&self.object_buffer, 0, &scratch);
        }
        self.scratch = scratch;

        self.draw_shadow_passes(encoder, shadows, &shadow_batches, &light_render, &light_batches, &light_owner, &light_faces);

        let nearest = rs.filter == TextureFilter::Nearest;
        for b in &batches {
            if b.normal_map.is_some() || b.orm_map.is_some() {
                let key = (b.texture, b.normal_map, b.orm_map, nearest);
                if !self.normal_mapped_groups.contains_key(&key) {
                    let base = b.texture.map_or(&self.white.view, |t| &self.textures[t].view);
                    let group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some("spark normal-mapped material"),
                        layout: &self.texture_layout,
                        entries: &[
                            wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(base) },
                            wgpu::BindGroupEntry {
                                binding: 1,
                                resource: wgpu::BindingResource::Sampler(&self.texture_samplers[if nearest { 0 } else { 1 }]),
                            },
                            wgpu::BindGroupEntry {
                                binding: 2,
                                resource: wgpu::BindingResource::TextureView(b.normal_map.map_or(&self.flat_normal.normal, |n| &self.textures[n].view)),
                            },
                            wgpu::BindGroupEntry {
                                binding: 3,
                                resource: wgpu::BindingResource::TextureView(b.orm_map.map_or(&self.flat_normal.orm, |n| &self.textures[n].view)),
                            },
                        ],
                    });
                    self.normal_mapped_groups.insert(key, group);
                }
            }
        }

        if sky.is_some() {
            self.ensure_sky_pipeline(samples);
        }
        let target = self.scene_target.as_ref().expect("scene target"); // ensure_scene_target() was called
        let bg = scene.background.to_linear();
        {
            let (color_view, resolve, depth_view) = match &target.msaa {
                Some((_, cv, _, dv)) => (cv, Some(&target.color_view), dv),
                None => (&target.color_view, None, &target.depth_view),
            };
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("spark scene"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: color_view,
                    depth_slice: None,
                    resolve_target: resolve,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color { r: bg[0] as f64, g: bg[1] as f64, b: bg[2] as f64, a: 1.0 }),
                        store: if resolve.is_some() { wgpu::StoreOp::Discard } else { wgpu::StoreOp::Store },
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: depth_view,
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(0.0), store: wgpu::StoreOp::Store }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_bind_group(0, &self.frame_bind_group, &[]);
            if sky.is_some() {
                pass.set_pipeline(&self.sky_pipelines[&samples]); // ensure_sky_pipeline() above
                pass.draw(0..3, 0..1);
            }
            let (mut last_pipe, mut last_shader, mut last_mesh, mut last_texture) =
                (None, None, usize::MAX, None::<(Option<usize>, Option<usize>, Option<usize>)>);
            for b in &batches {
                if last_pipe != Some((b.shader, b.variant)) {
                    pass.set_pipeline(self.pipeline(b.shader, (self.scene_format, b.variant, samples)));
                    last_pipe = Some((b.shader, b.variant));
                }
                if last_shader != Some(b.shader) {
                    pass.set_bind_group(3, &self.shader(b.shader).resources, &[]);
                    last_shader = Some(b.shader);
                }
                pass.set_bind_group(1, &self.object_bind_group, &[b.offset]);
                let mesh = self.meshes[b.mesh].as_ref().expect("uploaded mesh"); // candidates filtered to have uploaded meshes
                if last_texture != Some((b.texture, b.normal_map, b.orm_map)) {
                    let group = if b.normal_map.is_some() || b.orm_map.is_some() {
                        &self.normal_mapped_groups[&(b.texture, b.normal_map, b.orm_map, nearest)] // created above
                    } else {
                        let tex = b.texture.map(|t| &self.textures[t]).unwrap_or(&self.white);
                        if nearest { &tex.nearest } else { &tex.linear }
                    };
                    pass.set_bind_group(2, group, &[]);
                    last_texture = Some((b.texture, b.normal_map, b.orm_map));
                }
                if last_mesh != b.mesh {
                    pass.set_vertex_buffer(0, mesh.vertices.slice(..));
                    pass.set_index_buffer(mesh.indices.slice(..), wgpu::IndexFormat::Uint32);
                    last_mesh = b.mesh;
                }
                pass.draw_indexed(0..mesh.index_count, 0, 0..b.count);
            }
        }
        self.batches = batches;
        self.shadow_batches = shadow_batches;
        self.light_batches = light_batches;
        self.light_batch_owner = light_owner;
        self.light_batch_faces = light_faces;

        self.resolve_depth(encoder, world, samples);

        // 2D "scene" layer: drawn into the 3D image, so it goes through the post passes.
        self.draw_scene_canvas(encoder, world);
        stats
    }
}

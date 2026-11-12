//! The post chain: built-in effects, game post shaders, upscale and the 2D "ui" layer.

use spark_core::{
    Layer, PassSize,
    ShaderKind, TextureFilter, World,
};

use crate::gpu_util::*;
use crate::renderer::*;
use crate::resources::*;
use crate::uniforms::*;

impl Renderer {
    /// Post chain: the game's passes in order, ending at the output (a plain upscale is added when
    /// needed), then the 2D "ui" layer. Returns the number of game passes run.
    pub(crate) fn draw_post(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        format: wgpu::TextureFormat,
        world: &World,
    ) -> u32 {
        let output = (self.width, self.height);
        let scene_size = self.scene_target.as_ref().expect("scene target").size; // ensure_scene_target() was called
        let mut chain: Vec<(ShaderRef, (u32, u32))> = Vec::new();

        // Built-in post effects (run before user passes, at scene resolution).
        let passes = world.render.postfx.passes();
        self.update_postfx_params(&passes);
        chain.extend(passes.iter().map(|(kind, _)| (ShaderRef::Fx(*kind), scene_size)));

        // User-defined post passes.
        for p in &world.render.post {
            let Some(r) = self.asset_shader(Some(p.shader), ShaderKind::Post) else { continue };
            chain.push((r, if p.size == PassSize::Scene { scene_size } else { output }));
        }
        if chain.last().is_none_or(|l| l.1 != output) {
            chain.push((ShaderRef::Copy, output));
        }
        // Pipelines: intermediate passes render to the scene format, the last one to `format`.
        let n = chain.len();
        let mut keep = Vec::with_capacity(n);
        for (i, (r, _)) in chain.iter().enumerate() {
            let f = if i + 1 == n { format } else { self.scene_format };
            keep.push(self.ensure_shader_pipeline(*r, (f, 0, 1)));
        }
        let mut chain: Vec<_> = chain.into_iter().zip(keep).filter(|(_, ok)| *ok).map(|(c, _)| c).collect();
        if chain.last().is_none_or(|l| l.1 != output || !self.shader(l.0).pipelines.get(&(format, 0, 1)).is_some_and(|p| p.is_some())) {
            self.ensure_shader_pipeline(ShaderRef::Copy, (format, 0, 1));
            chain.push((ShaderRef::Copy, output));
        }
        let n = chain.len();
        let game_passes = chain.iter().filter(|c| c.0 != ShaderRef::Copy).count() as u32;

        // Intermediate images and per-pass info buffers.
        let max = self.device.limits().max_texture_dimension_2d;
        for i in 0..n - 1 {
            let size = (chain[i].1.0.clamp(1, max), chain[i].1.1.clamp(1, max));
            if self.post_targets.get(i).is_none_or(|t| t.size != size) {
                let texture = self.device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("spark post target"),
                    size: extent(size),
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: self.scene_format,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                });
                let t = PostTarget { size, view: texture.create_view(&Default::default()), _texture: texture };
                if i < self.post_targets.len() {
                    self.post_targets[i] = t;
                } else {
                    self.post_targets.push(t);
                }
            }
        }
        while self.pass_buffers.len() < n {
            self.pass_buffers.push(uniform_buffer(&self.device, "spark pass info", size_of::<PassUniform>() as u64));
        }
        let sampler = match world.render.upscale {
            TextureFilter::Nearest => &self.texture_samplers[2],
            TextureFilter::Linear => &self.texture_samplers[3],
        };
        let target = self.scene_target.as_ref().expect("scene target"); // ensure_scene_target() was called
        let mut input_size = scene_size;
        for (i, &(r, size)) in chain.iter().enumerate() {
            let last = i + 1 == n;
            let info = PassUniform {
                input_size: [input_size.0 as f32, input_size.1 as f32, 0.0, 0.0],
                output_size: [size.0 as f32, size.1 as f32, 0.0, 0.0],
            };
            self.queue.write_buffer(&self.pass_buffers[i], 0, bytemuck::bytes_of(&info));
            let input = if i == 0 { &target.color_view } else { &self.post_targets[i - 1].view };
            let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("spark post"),
                layout: &self.post_layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: self.frame_buffer.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(input) },
                    wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(sampler) },
                    wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&target.depth_view) },
                    wgpu::BindGroupEntry { binding: 4, resource: self.pass_buffers[i].as_entire_binding() },
                ],
            });
            let out = if last { view } else { &self.post_targets[i].view };
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("spark post"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: out,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(self.pipeline(r, (if last { format } else { self.scene_format }, 0, 1)));
            pass.set_bind_group(0, &bind_group, &[]);
            pass.set_bind_group(1, &self.shader(r).resources, &[]);
            pass.draw(0..3, 0..1);
            if last {
                // 2D "ui" layer: crisp, at full output resolution.
                self.draw_canvas_batches(&mut pass, format, &world.canvas.layer_data(Layer::Ui).batches, self.canvas_ui_base);
            }
            input_size = size;
        }
        game_passes
    }
}

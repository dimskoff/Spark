//! The 2D canvas: vertex upload, pipelines and layer drawing.

use spark_core::{
    Batch, Canvas, CanvasTexture, Layer, Vertex2D, World,
};

use crate::assets::*;
use crate::consts::*;
use crate::gpu_util::*;
use crate::renderer::*;

impl Renderer {
    pub(crate) fn prepare_canvas(&mut self, canvas: &Canvas) {
        let scene = canvas.layer_data(Layer::Scene);
        let ui = canvas.layer_data(Layer::Ui);
        self.canvas_ui_base = scene.vertices.len() as u32;
        if canvas.is_empty() {
            return;
        }
        let size = canvas.size();
        let uniform: [f32; 4] = [2.0 / size.x, -2.0 / size.y, -1.0, 1.0];
        self.queue.write_buffer(&self.canvas_buffer, 0, bytemuck::bytes_of(&uniform));

        let stride = size_of::<Vertex2D>() as u64;
        let scene_bytes = scene.vertices.len() as u64 * stride;
        let total = scene_bytes + ui.vertices.len() as u64 * stride;
        if total > self.canvas_vertices.size() {
            self.canvas_vertices = vertex_buffer(&self.device, total.next_power_of_two());
        }
        if !scene.vertices.is_empty() {
            self.queue.write_buffer(&self.canvas_vertices, 0, bytemuck::cast_slice(&scene.vertices));
        }
        if !ui.vertices.is_empty() {
            self.queue.write_buffer(&self.canvas_vertices, scene_bytes, bytemuck::cast_slice(&ui.vertices));
        }

        let fonts = &canvas.fonts;
        if fonts.version() != self.glyph_version {
            let atlas = fonts.atlas();
            match &self.glyphs {
                Some(g) if g.texture.width() == atlas.width && g.texture.height() == atlas.height => {
                    self.queue.write_texture(
                        wgpu::TexelCopyTextureInfo {
                            texture: &g.texture,
                            mip_level: 0,
                            origin: wgpu::Origin3d::ZERO,
                            aspect: wgpu::TextureAspect::All,
                        },
                        &atlas.pixels,
                        wgpu::TexelCopyBufferLayout {
                            offset: 0,
                            bytes_per_row: Some(atlas.width * 4),
                            rows_per_image: Some(atlas.height),
                        },
                        extent((atlas.width, atlas.height)),
                    );
                }
                _ => {
                    self.glyphs =
                        Some(upload_texture(&self.device, &self.queue, &self.texture_layout, &self.texture_samplers, &self.flat_normal, atlas, GLYPH_FORMAT, false));
                }
            }
            self.glyph_version = fonts.version();
        }
    }

    /// Draws 2D batches into an open pass. `base` = offset of the layer in `canvas_vertices`.
    pub(crate) fn draw_canvas_batches(&self, pass: &mut wgpu::RenderPass<'_>, format: wgpu::TextureFormat, batches: &[Batch], base: u32) {
        if batches.is_empty() {
            return;
        }
        pass.set_pipeline(&self.canvas_pipelines[&format]);
        pass.set_bind_group(0, &self.canvas_bind_group, &[]);
        pass.set_vertex_buffer(0, self.canvas_vertices.slice(..));
        for b in batches {
            let tex = match b.texture {
                CanvasTexture::White => &self.white,
                CanvasTexture::Texture(id) => self.textures.get(id.0 as usize).unwrap_or(&self.white),
                CanvasTexture::Glyphs => self.glyphs.as_ref().unwrap_or(&self.white),
            };
            pass.set_bind_group(1, if b.nearest { &tex.nearest_clamp } else { &tex.linear_clamp }, &[]);
            pass.draw(base + b.start..base + b.start + b.count, 0..1);
        }
    }

    pub(crate) fn ensure_canvas_pipeline(&mut self, format: wgpu::TextureFormat) {
        if !self.canvas_pipelines.contains_key(&format) {
            let p = canvas_pipeline(&self.device, &self.canvas_pipeline_layout, &self.canvas_shader, format);
            self.canvas_pipelines.insert(format, p);
        }
    }

    /// 2D "scene" layer: drawn into the 3D image, so it goes through the post passes.
    pub(crate) fn draw_scene_canvas(&mut self, encoder: &mut wgpu::CommandEncoder, world: &World) {
        self.prepare_canvas(&world.canvas);
        let scene_layer = world.canvas.layer_data(Layer::Scene);
        if !scene_layer.is_empty() {
            let target = self.scene_target.as_ref().expect("scene target"); // ensure_scene_target() was called
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("spark canvas scene"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target.color_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            self.draw_canvas_batches(&mut pass, self.scene_format, &scene_layer.batches, 0);
        }
    }
}

pub(crate) fn canvas_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    shader: &wgpu::ShaderModule,
    format: wgpu::TextureFormat,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("spark canvas"),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("vs_main"),
            compilation_options: Default::default(),
            buffers: &[Some(wgpu::VertexBufferLayout {
                array_stride: size_of::<Vertex2D>() as u64,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &CANVAS_ATTRIBUTES,
            })],
        },
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: Default::default(),
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some("fs_main"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    })
}

//! The scene render target (MSAA, depth), the sky pipeline and MSAA sample selection.

use spark_core::World;

use crate::consts::*;
use crate::gpu_util::*;
use crate::renderer::*;
use crate::resources::*;

impl Renderer {
    /// Skybox pipeline for the scene pass at `samples` MSAA (depth test off, no depth write).
    pub(crate) fn ensure_sky_pipeline(&mut self, samples: u32) {
        if self.sky_pipelines.contains_key(&samples) {
            return;
        }
        let p = self.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("spark sky"),
            layout: Some(&self.sky_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &self.sky_shader,
                entry_point: Some("sky_vs"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: Default::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::Always),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: wgpu::MultisampleState { count: samples, ..Default::default() },
            fragment: Some(wgpu::FragmentState {
                module: &self.sky_shader,
                entry_point: Some("sky_fs"),
                compilation_options: Default::default(),
                targets: &[Some(self.scene_format.into())],
            }),
            multiview_mask: None,
            cache: None,
        });
        self.sky_pipelines.insert(samples, p);
    }

    pub(crate) fn ensure_scene_target(&mut self, size: (u32, u32), samples: u32) {
        let max = self.device.limits().max_texture_dimension_2d;
        let size = (size.0.clamp(1, max), size.1.clamp(1, max));
        if self.scene_target.as_ref().is_some_and(|t| t.size == size && t.samples == samples) {
            return;
        }
        let make = |label: &str, format: wgpu::TextureFormat, usage: wgpu::TextureUsages, sample_count: u32| {
            self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: extent(size),
                mip_level_count: 1,
                sample_count,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage,
                view_formats: &[],
            })
        };
        let rt = wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING;
        let color = make("spark scene color", self.scene_format, rt, 1);
        let depth = make("spark scene depth", DEPTH_FORMAT, rt, 1);
        let (msaa, depth_resolve) = if samples > 1 {
            let mc = make("spark scene color msaa", self.scene_format, wgpu::TextureUsages::RENDER_ATTACHMENT, samples);
            let md = make("spark scene depth msaa", DEPTH_FORMAT, rt, samples);
            let (mcv, mdv) = (mc.create_view(&Default::default()), md.create_view(&Default::default()));
            let bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("spark depth resolve"),
                layout: &self.depth_resolve_layout,
                entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&mdv) }],
            });
            (Some((mc, mcv, md, mdv)), Some(bg))
        } else {
            (None, None)
        };
        self.scene_target = Some(SceneTarget {
            size,
            samples,
            color_view: color.create_view(&Default::default()),
            depth_view: depth.create_view(&Default::default()),
            _color: color,
            _depth: depth,
            msaa,
            depth_resolve,
        });
    }

    /// Uploads the 2D vertices of both layers, the screen transform and the glyph atlas.

    /// MSAA sample count supported for both scene formats (falls back to 1).
    pub(crate) fn supported_samples(&self, wanted: u32) -> u32 {
        let ok = |n: u32| {
            [self.scene_format, DEPTH_FORMAT].iter().all(|f| self.adapter_formats.get(f).is_some_and(|fl| fl.sample_count_supported(n)))
        };
        if self.info.backend == "Gl" {
            return 1; // GL / GLES: no multisampled depth reads for the post chain
        }
        [8, 4, 2].into_iter().find(|&n| n <= wanted && ok(n)).unwrap_or(1)
    }

    /// Post shaders and depth-based built-in effects (SSAO, depth of field, motion blur) read the single-sample
    /// depth: with MSAA it is resolved here, and only when one of them will run.
    pub(crate) fn resolve_depth(&mut self, encoder: &mut wgpu::CommandEncoder, world: &World, samples: u32) {
        if samples > 1 && (!world.render.post.is_empty() || world.render.postfx.is_active()) {
            if !self.depth_resolve_pipelines.contains_key(&samples) {
                let p = self.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some("spark depth resolve"),
                    layout: Some(&self.depth_resolve_layout_p),
                    vertex: wgpu::VertexState {
                        module: &self.depth_resolve_shader,
                        entry_point: Some("vs_main"),
                        compilation_options: Default::default(),
                        buffers: &[],
                    },
                    primitive: Default::default(),
                    depth_stencil: Some(wgpu::DepthStencilState {
                        format: DEPTH_FORMAT,
                        depth_write_enabled: Some(true),
                        depth_compare: Some(wgpu::CompareFunction::Always),
                        stencil: Default::default(),
                        bias: Default::default(),
                    }),
                    multisample: Default::default(),
                    fragment: Some(wgpu::FragmentState {
                        module: &self.depth_resolve_shader,
                        entry_point: Some("fs_main"),
                        compilation_options: Default::default(),
                        targets: &[],
                    }),
                    multiview_mask: None,
                    cache: None,
                });
                self.depth_resolve_pipelines.insert(samples, p);
            }
            let target = self.scene_target.as_ref().expect("scene target"); // ensure_scene_target() was called
            if let Some(bg) = &target.depth_resolve {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("spark depth resolve"),
                    color_attachments: &[],
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                        view: &target.depth_view,
                        depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(0.0), store: wgpu::StoreOp::Store }),
                        stencil_ops: None,
                    }),
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                pass.set_pipeline(&self.depth_resolve_pipelines[&samples]);
                pass.set_bind_group(0, bg, &[]);
                pass.draw(0..3, 0..1);
            }
        }
    }
}

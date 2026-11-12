//! Game and built-in shaders on the GPU: modules, pipelines, parameters.

use std::collections::HashMap;

use spark_core::shader::MAX_PARAMS_SIZE;
use spark_core::{
    Assets, ShaderData,
    ShaderKind, TextureId, Vertex,
};

use crate::consts::*;
use crate::gpu_util::*;
use crate::renderer::*;
use crate::resources::*;

impl Renderer {
    /// GPU copies of the game's shaders: (re)built when added / hot-reloaded, params uploaded every frame.
    pub(crate) fn sync_shaders(&mut self, assets: &Assets) {
        for i in 0..assets.shader_count() {
            // loop bound is assets.shader_count(), so id is in range.
            let data = assets.shader(spark_core::ShaderId(i as u32)).expect("shader id in range");
            if self.shaders.get(i).is_none_or(|s| s.version != data.version) {
                let g = gpu_shader(&self.device, &self.shader_layout, &self.texture_samplers, &self.white, &self.textures, data);
                if i < self.shaders.len() {
                    self.shaders[i] = g;
                } else {
                    self.shaders.push(g);
                }
            } else if self.shaders[i].textures != data.textures {
                let s = &mut self.shaders[i];
                s.resources = shader_resources(&self.device, &self.shader_layout, &self.texture_samplers, &self.white, &self.textures, &s.params, data.textures);
                s.textures = data.textures;
            }
            self.queue.write_buffer(&self.shaders[i].params, 0, &data.values);
        }
    }

    /// Uploads the parameters of the enabled built-in effects. A name mismatch is an engine bug
    /// (covered by tests); it is logged instead of panicking a running game.
    pub(crate) fn update_postfx_params(&mut self, passes: &[(spark_core::shader::PostFxKind, spark_core::render::PostFxParams)]) {
        for (kind, params) in passes {
            let i = *kind as usize;
            let data = &mut self.built_in_post_data[i];
            for (name, value) in params {
                if let Err(e) = data.set_param(name, value) {
                    log::error!("{e}");
                }
            }
            self.queue.write_buffer(&self.built_in_post[i].params, 0, &data.values);
        }
    }

    pub(crate) fn shader(&self, r: ShaderRef) -> &GpuShader {
        match r {
            ShaderRef::Default => &self.default_surface,
            ShaderRef::Copy => &self.copy_post,
            ShaderRef::Asset(i) => &self.shaders[i],
            ShaderRef::Fx(k) => &self.built_in_post[k as usize],
        }
    }

    /// Asset shader `id` if it exists and is of `kind`.
    pub(crate) fn asset_shader(&self, id: Option<spark_core::ShaderId>, kind: ShaderKind) -> Option<ShaderRef> {
        let i = id?.0 as usize;
        (self.shaders.get(i)?.kind == kind).then_some(ShaderRef::Asset(i))
    }

    /// Builds the pipeline of `r` for `format` if needed. `false` = the shader cannot be used.
    pub(crate) fn ensure_shader_pipeline(&mut self, r: ShaderRef, key: PipelineKey) -> bool {
        let s = match r {
            ShaderRef::Default => &mut self.default_surface,
            ShaderRef::Copy => &mut self.copy_post,
            ShaderRef::Asset(i) => &mut self.shaders[i],
            ShaderRef::Fx(k) => &mut self.built_in_post[k as usize],
        };
        let layout = match s.kind {
            ShaderKind::Surface => &self.mesh_pipeline_layout,
            ShaderKind::Post => &self.post_pipeline_layout,
        };
        ensure_pipeline(&self.device, layout, s, key);
        s.pipelines.get(&key).is_some_and(|p| p.is_some())
    }

    pub(crate) fn pipeline(&self, r: ShaderRef, key: PipelineKey) -> &wgpu::RenderPipeline {
        // caller must ensure_shader_pipeline() before calling pipeline().
        self.shader(r).pipelines[&key].as_ref().expect("pipeline ensured")
    }
}

/// Uploads a shader to the GPU (its module is validated by the GPU here; a rejected shader logs an error).
pub(crate) fn gpu_shader(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    samplers: &[wgpu::Sampler; 4],
    white: &GpuTexture,
    textures: &[GpuTexture],
    data: &ShaderData,
) -> GpuShader {
    let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(&data.name),
        source: wgpu::ShaderSource::Wgsl(data.source.as_str().into()),
    });
    let module = match pollster::block_on(scope.pop()) {
        None => Some(module),
        Some(e) => {
            log::error!("shader {}: the GPU rejected it, using the default instead: {e}", data.name);
            None
        }
    };
    let params = uniform_buffer(device, "spark shader params", data.values.len().clamp(16, MAX_PARAMS_SIZE) as u64);
    let resources = shader_resources(device, layout, samplers, white, textures, &params, data.textures);
    GpuShader {
        name: data.name.clone(),
        kind: data.kind,
        version: data.version,
        module,
        pipelines: HashMap::new(),
        params,
        resources,
        textures: data.textures,
    }
}

pub(crate) fn shader_resources(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    samplers: &[wgpu::Sampler; 4],
    white: &GpuTexture,
    textures: &[GpuTexture],
    params: &wgpu::Buffer,
    bound: [Option<TextureId>; 2],
) -> wgpu::BindGroup {
    let view = |t: Option<TextureId>| t.and_then(|t| textures.get(t.0 as usize)).map(|g| &g.view).unwrap_or(&white.view);
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("spark shader resources"),
        layout,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: params.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(view(bound[0])) },
            wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(view(bound[1])) },
            wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::Sampler(&samplers[1]) },
            wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::Sampler(&samplers[0]) },
        ],
    })
}

/// Pipeline of a shader for one output format (cached; `None` cached when the GPU rejects it).
pub(crate) fn ensure_pipeline(device: &wgpu::Device, layout: &wgpu::PipelineLayout, s: &mut GpuShader, key: PipelineKey) {
    if s.pipelines.contains_key(&key) {
        return;
    }
    let (format, variant, samples) = key;
    let Some(module) = &s.module else {
        s.pipelines.insert(key, None);
        return;
    };
    let transparent = variant & (VARIANT_ALPHA | VARIANT_ADDITIVE) != 0;
    let blend = if variant & VARIANT_ADDITIVE != 0 {
        Some(wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::SrcAlpha,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::Zero,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            },
        })
    } else if variant & VARIANT_ALPHA != 0 {
        Some(wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::SrcAlpha,
                dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent::OVER,
        })
    } else {
        None
    };
    let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let vertex_buffers = [Some(wgpu::VertexBufferLayout {
        array_stride: size_of::<Vertex>() as u64,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &VERTEX_ATTRIBUTES,
    })];
    let surface = s.kind == ShaderKind::Surface;
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(&s.name),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module,
            entry_point: Some("spark_vs"),
            compilation_options: Default::default(),
            buffers: if surface { &vertex_buffers } else { &[] },
        },
        primitive: if surface {
            wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: if variant & VARIANT_DOUBLE_SIDED != 0 { None } else { Some(wgpu::Face::Back) },
                ..Default::default()
            }
        } else {
            wgpu::PrimitiveState::default()
        },
        depth_stencil: surface.then(|| wgpu::DepthStencilState {
            format: DEPTH_FORMAT,
            depth_write_enabled: Some(!transparent),
            depth_compare: Some(wgpu::CompareFunction::Greater),
            stencil: Default::default(),
            bias: Default::default(),
        }),
        multisample: wgpu::MultisampleState { count: samples, ..Default::default() },
        fragment: Some(wgpu::FragmentState {
            module,
            entry_point: Some("spark_fs"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState { format, blend, write_mask: wgpu::ColorWrites::ALL })],
        }),
        multiview_mask: None,
        cache: None,
    });
    let pipeline = match pollster::block_on(scope.pop()) {
        None => Some(pipeline),
        Some(e) => {
            log::error!("shader {}: cannot build its pipeline, using the default instead: {e}", s.name);
            None
        }
    };
    s.pipelines.insert(key, pipeline);
}

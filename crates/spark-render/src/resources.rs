//! GPU-side resources owned by the renderer (meshes, textures, targets, shadow maps) and per-frame draw records.

use std::collections::HashMap;

use spark_core::{ShaderKind, TextureId, Vec3};

use crate::consts::*;
use crate::uniforms::*;

pub(crate) struct GpuMesh {
    pub(crate) vertices: wgpu::Buffer,
    pub(crate) indices: wgpu::Buffer,
    pub(crate) index_count: u32,
    pub(crate) vertex_count: usize,
    /// `Assets::mesh_version` when uploaded.
    pub(crate) version: u64,
}

/// Sky cubemap on the GPU (full mip chain: blurrier mips light rough surfaces).
pub(crate) struct GpuCube {
    pub(crate) view: wgpu::TextureView,
    pub(crate) mips: u32,
}

pub(crate) struct GpuTexture {
    pub(crate) texture: wgpu::Texture,
    pub(crate) view: wgpu::TextureView,
    pub(crate) nearest: wgpu::BindGroup,
    pub(crate) linear: wgpu::BindGroup,
    pub(crate) nearest_clamp: wgpu::BindGroup,
    pub(crate) linear_clamp: wgpu::BindGroup,
}

/// A shader on the GPU: module, its `Params` buffer + resources, pipelines per output format.
pub(crate) struct GpuShader {
    pub(crate) name: String,
    pub(crate) kind: ShaderKind,
    pub(crate) version: u64,
    /// `None` when the GPU rejected it (logged once; the default shader is used instead).
    pub(crate) module: Option<wgpu::ShaderModule>,
    pub(crate) pipelines: HashMap<PipelineKey, Option<wgpu::RenderPipeline>>,
    pub(crate) params: wgpu::Buffer,
    pub(crate) resources: wgpu::BindGroup,
    pub(crate) textures: [Option<TextureId>; 2],
}

/// Which shader draws something.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub(crate) enum ShaderRef {
    Default,
    Copy,
    Asset(usize),
    /// Built-in post-processing effect.
    Fx(spark_core::shader::PostFxKind),
}

pub(crate) struct SceneTarget {
    pub(crate) size: (u32, u32),
    pub(crate) samples: u32,
    pub(crate) _color: wgpu::Texture,
    pub(crate) _depth: wgpu::Texture,
    /// Single-sample (resolved) scene image and depth: read by the post chain.
    pub(crate) color_view: wgpu::TextureView,
    pub(crate) depth_view: wgpu::TextureView,
    /// Multisampled color + depth when MSAA is on (rendered into, then resolved).
    pub(crate) msaa: Option<(wgpu::Texture, wgpu::TextureView, wgpu::Texture, wgpu::TextureView)>,
    /// Bind group reading the multisampled depth (depth resolve pass).
    pub(crate) depth_resolve: Option<wgpu::BindGroup>,
}

/// Sun shadow map.
pub(crate) struct ShadowMap {
    pub(crate) size: u32,
    pub(crate) _texture: wgpu::Texture,
    pub(crate) view: wgpu::TextureView,
}

/// Shadow cubes of point / spot lights: a depth array, six layers per slot.
pub(crate) struct LightShadowMap {
    pub(crate) size: u32,
    pub(crate) _texture: wgpu::Texture,
    /// All layers (sampled by surface shaders).
    pub(crate) view: wgpu::TextureView,
    /// One view per layer (render targets).
    pub(crate) layers: Vec<wgpu::TextureView>,
}

/// The light that owns a shadow slot. `fresh` = its cube was rendered for this position / range.
#[derive(Clone, Copy)]
pub(crate) struct LightSlot {
    pub(crate) id: spark_core::ObjectId,
    pub(crate) position: Vec3,
    pub(crate) range: f32,
    pub(crate) fresh: bool,
}

/// A light that asked for shadows this frame (`collect_lights`): its entry in the frame's light list.
pub(crate) struct ShadowRequest {
    pub(crate) distance: f32,
    pub(crate) id: spark_core::ObjectId,
    pub(crate) position: Vec3,
    pub(crate) range: f32,
    pub(crate) light: usize,
}

/// One object to draw this frame (before sorting / batching).
pub(crate) struct Candidate {
    pub(crate) id: spark_core::ObjectId,
    pub(crate) uniform: ObjectUniform,
    pub(crate) shader: ShaderRef,
    pub(crate) variant: u8,
    pub(crate) mesh: usize,
    pub(crate) texture: Option<usize>,
    pub(crate) normal_map: Option<usize>,
    /// View-space distance (transparent sorting).
    pub(crate) depth: f32,
    pub(crate) in_view: bool,
    pub(crate) caster: bool,
    /// Casts into the shadow cube of a light rendered this frame.
    pub(crate) light_caster: bool,
    /// World bounding sphere.
    pub(crate) center: Vec3,
    pub(crate) radius: f32,
    pub(crate) triangles: u64,
}

/// Consecutive objects drawn with one instanced draw call.
pub(crate) struct Batch3 {
    pub(crate) shader: ShaderRef,
    pub(crate) variant: u8,
    pub(crate) mesh: usize,
    pub(crate) texture: Option<usize>,
    pub(crate) normal_map: Option<usize>,
    /// Byte offset of the batch's first object (dynamic uniform offset).
    pub(crate) offset: u32,
    pub(crate) count: u32,
}

/// Intermediate image of the post chain.
pub(crate) struct PostTarget {
    pub(crate) size: (u32, u32),
    pub(crate) _texture: wgpu::Texture,
    pub(crate) view: wgpu::TextureView,
}

pub(crate) struct Capture {
    pub(crate) size: (u32, u32),
    pub(crate) texture: wgpu::Texture,
    pub(crate) view: wgpu::TextureView,
}

pub(crate) struct SurfaceState {
    pub(crate) surface: wgpu::Surface<'static>,
    pub(crate) config: wgpu::SurfaceConfiguration,
    pub(crate) view_format: wgpu::TextureFormat,
}

pub(crate) type SurfaceFactory = Box<dyn Fn(&wgpu::Instance) -> Option<wgpu::Surface<'static>>>;

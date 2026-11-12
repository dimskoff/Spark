//! CPU mirrors of the uniform blocks declared in the WGSL shaders (layout must match).

use bytemuck::{Pod, Zeroable};

/// Matches `struct Frame` in spark-core/src/shaders/common.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub(crate) struct FrameUniform {
    pub(crate) view_proj: [[f32; 4]; 4],
    pub(crate) view: [[f32; 4]; 4],
    pub(crate) proj: [[f32; 4]; 4],
    pub(crate) inv_view_proj: [[f32; 4]; 4],
    pub(crate) camera_pos: [f32; 4],
    pub(crate) camera: [f32; 4],
    pub(crate) screen: [f32; 4],
    pub(crate) time: [f32; 4],
    pub(crate) sun_dir: [f32; 4],
    pub(crate) sun_color: [f32; 4],
    pub(crate) ambient: [f32; 4],
    pub(crate) fog_color: [f32; 4],
    pub(crate) fog: [f32; 4],
    pub(crate) light_count: [u32; 4],
    pub(crate) lights: [LightUniform; MAX_LIGHTS],
    pub(crate) shadow_view_proj: [[f32; 4]; 4],
    /// enabled, 1 / map size, normal offset, shadow distance
    pub(crate) shadow: [f32; 4],
    pub(crate) prev_view_proj: [[f32; 4]; 4],
    pub(crate) sky_top: [f32; 4],
    pub(crate) sky_bottom: [f32; 4],
    /// enabled, cubemap blend, lighting strength, cubemap mip count
    pub(crate) sky: [f32; 4],
    /// world -> clip of each shadow cube face: slot * 6 + face (+X, -X, +Y, -Y, +Z, -Z).
    pub(crate) light_shadow_view_proj: [[[f32; 4]; 4]; SHADOW_FACES],
    /// enabled, 1 / face size, -, -
    pub(crate) light_shadow: [f32; 4],
}

pub(crate) const MAX_LIGHTS: usize = spark_core::Light::MAX_VISIBLE;
/// Point / spot lights with a shadow cube: six faces each, layers of one depth texture array.
pub(crate) const MAX_SHADOWED: usize = spark_core::Light::MAX_SHADOWED;
/// Must match the array length of `light_shadow_view_proj` in `common.wgsl`.
pub(crate) const SHADOW_FACES: usize = MAX_SHADOWED * 6;
/// Near plane of a light's shadow cube, meters.
pub(crate) const LIGHT_SHADOW_NEAR: f32 = 0.05;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub(crate) struct LightUniform {
    /// xyz = world position, w = range.
    pub(crate) position: [f32; 4],
    /// rgb = linear color * intensity, w = 1 for spot.
    pub(crate) color: [f32; 4],
    /// xyz = direction, w = cos(outer half angle).
    pub(crate) direction: [f32; 4],
    /// x = cos(inner half angle), y = shadow slot + 1 (0 = none).
    pub(crate) params: [f32; 4],
}

/// Matches `struct Object` in spark-core/src/shaders/surface.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub(crate) struct ObjectUniform {
    pub(crate) model: [[f32; 4]; 4],
    pub(crate) normal_matrix: [[f32; 4]; 4],
    pub(crate) color: [f32; 4],
    pub(crate) data: [f32; 4],
    /// tiling x, tiling y, unlit, pbr
    pub(crate) info: [f32; 4],
    /// metallic, roughness, has normal map, -
    pub(crate) pbr: [f32; 4],
    /// rgb (linear), -
    pub(crate) emissive: [f32; 4],
}

/// Matches `struct PassInfo` in spark-core/src/shaders/post.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub(crate) struct PassUniform {
    pub(crate) input_size: [f32; 4],
    pub(crate) output_size: [f32; 4],
}

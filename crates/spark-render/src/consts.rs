//! Formats and limits shared by the whole renderer.

/// Scene depth: float, reversed (near = 1, far = 0, see `Camera::render_projection`).
pub(crate) const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
pub(crate) const COLOR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
/// Scene and intermediate post images when the GPU supports it (see `Renderer::scene_format`).
pub(crate) const HDR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
pub(crate) const VERTEX_ATTRIBUTES: [wgpu::VertexAttribute; 3] =
    wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x2];
pub(crate) const CANVAS_ATTRIBUTES: [wgpu::VertexAttribute; 3] =
    wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2, 2 => Float32x4];
/// Glyph atlas: coverage in alpha, not sRGB-encoded.
pub(crate) const GLYPH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
pub(crate) const SHADOW_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

/// Objects per instanced draw: `array<Object, 64>` uniform window (fits every GPU incl. GL).
/// Must match the array length in `surface.wgsl` and `shadow.wgsl`.
pub(crate) const MAX_INSTANCES: usize = 64;
/// Pipeline variant bits: blend mode (0 opaque, 1 alpha, 2 additive) | double-sided.
pub(crate) const VARIANT_ALPHA: u8 = 1;
pub(crate) const VARIANT_ADDITIVE: u8 = 2;
pub(crate) const VARIANT_DOUBLE_SIDED: u8 = 4;

/// Pipeline cache key: output format, variant bits, MSAA sample count.
pub(crate) type PipelineKey = (wgpu::TextureFormat, u8, u32);

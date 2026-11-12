//! Small wgpu helpers: bind group layouts, buffers, extents.

use std::num::NonZeroU64;

use spark_core::Mat4;

use crate::consts::*;
use crate::uniforms::*;
pub(crate) fn bind_group_layout(device: &wgpu::Device, label: &str, entries: &[wgpu::BindGroupLayoutEntry]) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some(label), entries })
}

pub(crate) fn uniform_entry(binding: u32, dynamic: bool, size: u64, visibility: wgpu::ShaderStages) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: dynamic,
            min_binding_size: NonZeroU64::new(size),
        },
        count: None,
    }
}

pub(crate) fn texture_entry(binding: u32, visibility: wgpu::ShaderStages) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: true },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    }
}

pub(crate) fn sampler_entry(binding: u32, visibility: wgpu::ShaderStages) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility,
        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
        count: None,
    }
}

pub(crate) fn uniform_buffer(device: &wgpu::Device, label: &str, size: u64) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

pub(crate) fn object_buffer(device: &wgpu::Device, layout: &wgpu::BindGroupLayout, capacity: usize) -> (wgpu::Buffer, wgpu::BindGroup) {
    let buffer = uniform_buffer(device, "spark objects", capacity as u64);
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("spark objects"),
        layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                buffer: &buffer,
                offset: 0,
                size: NonZeroU64::new((MAX_INSTANCES * size_of::<ObjectUniform>()) as u64),
            }),
        }],
    });
    (buffer, bind_group)
}

pub(crate) fn frame_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    buffer: &wgpu::Buffer,
    shadow: &wgpu::TextureView,
    sampler: &wgpu::Sampler,
    sky: &wgpu::TextureView,
    sky_sampler: &wgpu::Sampler,
    light_shadow: &wgpu::TextureView,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("spark frame"),
        layout,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: buffer.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(shadow) },
            wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(sampler) },
            wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(sky) },
            wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::Sampler(sky_sampler) },
            wgpu::BindGroupEntry { binding: 5, resource: wgpu::BindingResource::TextureView(light_shadow) },
        ],
    })
}

pub(crate) fn vertex_buffer(device: &wgpu::Device, size: u64) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("spark canvas vertices"),
        size,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

pub(crate) fn extent(size: (u32, u32)) -> wgpu::Extent3d {
    wgpu::Extent3d { width: size.0, height: size.1, depth_or_array_layers: 1 }
}

pub(crate) fn flag(value: bool) -> f32 {
    if value { 1.0 } else { 0.0 }
}

pub(crate) fn normal_matrix(model: &Mat4) -> Mat4 {
    if model.determinant().abs() < 1e-12 { Mat4::IDENTITY } else { model.inverse().transpose() }
}

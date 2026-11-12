//! Uploads `Assets` (meshes, textures, cubemaps) to the GPU and keeps them in sync.

use spark_core::{
    Assets, Color, CubemapId, ImageData, MeshId, TextureId,
};
use wgpu::util::DeviceExt;

use crate::consts::*;
use crate::gpu_util::*;
use crate::renderer::*;
use crate::resources::*;

impl Renderer {
    /// Uploads meshes/textures added to `assets` since the last frame.
    pub(crate) fn sync_assets(&mut self, assets: &Assets) {
        if assets.generation() != self.assets_generation {
            self.meshes.clear();
            self.textures.clear();
            self.cubemaps.clear();
            self.normal_mapped_groups.clear();
            self.shaders.clear();
            self.assets_generation = assets.generation();
        }
        while self.meshes.len() < assets.mesh_count() {
            // loop condition guarantees mesh id < assets.mesh_count().
            let data = assets.mesh(MeshId(self.meshes.len() as u32)).expect("mesh id in range");
            let gpu = (!data.vertices.is_empty() && !data.indices.is_empty()).then(|| GpuMesh {
                vertices: self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("spark vertices"),
                    contents: bytemuck::cast_slice(&data.vertices),
                    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                }),
                indices: self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("spark indices"),
                    contents: bytemuck::cast_slice(&data.indices),
                    usage: wgpu::BufferUsages::INDEX,
                }),
                index_count: data.indices.len() as u32,
                vertex_count: data.vertices.len(),
                version: assets.mesh_version(MeshId(self.meshes.len() as u32)),
            });
            self.meshes.push(gpu);
        }
        // Meshes edited in place (skinned models): rewrite their vertex buffers.
        if assets.mesh_edits() != self.mesh_edits {
            self.mesh_edits = assets.mesh_edits();
            for (i, slot) in self.meshes.iter_mut().enumerate() {
                let Some(gpu) = slot else { continue };
                let version = assets.mesh_version(MeshId(i as u32));
                if gpu.version == version {
                    continue;
                }
                gpu.version = version;
                if let Some(data) = assets.mesh(MeshId(i as u32)).filter(|d| d.vertices.len() == gpu.vertex_count) {
                    self.queue.write_buffer(&gpu.vertices, 0, bytemuck::cast_slice(&data.vertices));
                }
            }
        }
        let max = self.device.limits().max_texture_dimension_2d;
        while self.textures.len() < assets.texture_count() {
            // loop condition guarantees texture id < assets.texture_count().
            let image = assets.texture(TextureId(self.textures.len() as u32)).expect("texture id in range");
            let gpu = if image.width == 0 || image.height == 0 || image.width > max || image.height > max {
                log::error!("texture {}x{} is not supported (max {max}x{max}); using magenta", image.width, image.height);
                let magenta = ImageData::solid(Color::hex(0xff00ff));
                upload_texture(&self.device, &self.queue, &self.texture_layout, &self.texture_samplers, &self.flat_normal, &magenta, COLOR_FORMAT, false)
            } else {
                upload_texture(&self.device, &self.queue, &self.texture_layout, &self.texture_samplers, &self.flat_normal, image, COLOR_FORMAT, true)
            };
            self.textures.push(gpu);
        }
        while self.cubemaps.len() < assets.cubemap_count() {
            // loop condition guarantees cubemap id < assets.cubemap_count().
            let faces = assets.cubemap(CubemapId(self.cubemaps.len() as u32)).expect("cubemap id in range");
            let gpu = if faces[0].width > max {
                log::error!("cubemap faces {0}x{0} are too big (max {1}); using white", faces[0].width, max);
                upload_cube(&self.device, &self.queue, &std::array::from_fn(|_| ImageData::solid(Color::WHITE)))
            } else {
                upload_cube(&self.device, &self.queue, faces)
            };
            self.cubemaps.push(gpu);
        }
    }
}

/// Uploads six square faces (+X, -X, +Y, -Y, +Z, -Z) with a full mip chain each.
pub(crate) fn upload_cube(device: &wgpu::Device, queue: &wgpu::Queue, faces: &[ImageData; 6]) -> GpuCube {
    let size = faces[0].width;
    let mut mips = 1;
    let mut data = Vec::new();
    for face in faces {
        let (levels, bytes) = build_mips(face);
        mips = levels;
        data.extend_from_slice(&bytes);
    }
    let texture = device.create_texture_with_data(
        queue,
        &wgpu::TextureDescriptor {
            label: Some("spark sky cubemap"),
            size: wgpu::Extent3d { width: size, height: size, depth_or_array_layers: 6 },
            mip_level_count: mips,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: COLOR_FORMAT,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        },
        wgpu::util::TextureDataOrder::LayerMajor,
        &data,
    );
    let view = texture.create_view(&wgpu::TextureViewDescriptor {
        dimension: Some(wgpu::TextureViewDimension::Cube),
        ..Default::default()
    });
    GpuCube { view, mips }
}

pub(crate) fn upload_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    layout: &wgpu::BindGroupLayout,
    samplers: &[wgpu::Sampler; 4],
    flat: &crate::resources::FlatMaps,
    image: &ImageData,
    format: wgpu::TextureFormat,
    mipmaps: bool,
) -> GpuTexture {
    let (levels, data) = if mipmaps { build_mips(image) } else { (1, std::borrow::Cow::Borrowed(&image.pixels[..])) };
    let texture = device.create_texture_with_data(
        queue,
        &wgpu::TextureDescriptor {
            label: Some("spark texture"),
            size: extent((image.width, image.height)),
            mip_level_count: levels,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        },
        wgpu::util::TextureDataOrder::LayerMajor,
        &data,
    );
    let view = texture.create_view(&Default::default());
    let bind_group = |sampler: &wgpu::Sampler| {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("spark texture"),
            layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(sampler) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&flat.normal) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&flat.orm) },
            ],
        })
    };
    GpuTexture {
        nearest: bind_group(&samplers[0]),
        linear: bind_group(&samplers[1]),
        nearest_clamp: bind_group(&samplers[2]),
        linear_clamp: bind_group(&samplers[3]),
        texture,
        view,
    }
}

/// Full mip chain (box filter in linear space, sRGB RGBA8): kills moire on distant tiled surfaces.
pub(crate) fn build_mips(image: &ImageData) -> (u32, std::borrow::Cow<'_, [u8]>) {
    let (mut w, mut h) = (image.width as usize, image.height as usize);
    let levels = 32 - (w.max(h) as u32).leading_zeros();
    if levels <= 1 {
        return (1, std::borrow::Cow::Borrowed(&image.pixels[..]));
    }
    let to_lin: Vec<f32> = (0..256).map(|i| (i as f32 / 255.0).powf(2.2)).collect();
    let mut out = image.pixels.clone();
    let mut cur: Vec<f32> = image.pixels.chunks(4).flat_map(|p| [to_lin[p[0] as usize], to_lin[p[1] as usize], to_lin[p[2] as usize], p[3] as f32 / 255.0]).collect();
    for _ in 1..levels {
        let (nw, nh) = ((w / 2).max(1), (h / 2).max(1));
        let mut next = vec![0.0f32; nw * nh * 4];
        for y in 0..nh {
            for x in 0..nw {
                let mut acc = [0.0f32; 4];
                let mut wsum = 0.0;
                for (sx, sy) in [(2 * x, 2 * y), (2 * x + 1, 2 * y), (2 * x, 2 * y + 1), (2 * x + 1, 2 * y + 1)] {
                    let (sx, sy) = (sx.min(w - 1), sy.min(h - 1));
                    let p = &cur[(sy * w + sx) * 4..][..4];
                    // alpha-weighted color so transparent texels don't bleed dark fringes
                    let a = p[3].max(1e-4);
                    for c in 0..3 {
                        acc[c] += p[c] * a;
                    }
                    acc[3] += p[3];
                    wsum += a;
                }
                let o = &mut next[(y * nw + x) * 4..][..4];
                for c in 0..3 {
                    o[c] = acc[c] / wsum;
                }
                o[3] = acc[3] / 4.0;
            }
        }
        out.extend(next.chunks(4).flat_map(|p| {
            let e = |v: f32| (v.max(0.0).powf(1.0 / 2.2) * 255.0 + 0.5).min(255.0) as u8;
            [e(p[0]), e(p[1]), e(p[2]), (p[3] * 255.0 + 0.5).min(255.0) as u8]
        }));
        cur = next;
        (w, h) = (nw, nh);
    }
    (levels, std::borrow::Cow::Owned(out))
}

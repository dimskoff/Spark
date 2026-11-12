//! Screenshots and frame capture: render into an offscreen target and read the pixels back.

use spark_core::{
    ImageData, World,
};

use crate::consts::*;
use crate::gpu_util::*;
use crate::renderer::*;
use crate::resources::*;

impl Renderer {
    pub(crate) fn ensure_capture(&mut self) {
        let size = (self.width, self.height);
        if self.capture.as_ref().is_none_or(|c| c.size != size) {
            let texture = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("spark capture"),
                size: extent(size),
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: COLOR_FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            });
            let view = texture.create_view(&Default::default());
            self.capture = Some(Capture { size, texture, view });
        }
    }

    /// Draws the world and returns the final image (same as what the window shows).
    pub fn screenshot(&mut self, world: &World) -> Result<ImageData, String> {
        if self.is_device_lost() {
            return Err("GPU device lost".into());
        }
        self.ensure_capture();
        self.ensure_canvas_pipeline(COLOR_FORMAT);
        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("spark screenshot") });
        let mut stats = self.draw_scene(&mut encoder, world);
        // ensure_capture() was just called, so self.capture is Some.
        let capture = self.capture.take().ok_or_else(|| "capture target was not ensured".to_string())?;
        stats.post_passes = self.draw_post(&mut encoder, &capture.view, COLOR_FORMAT, world);
        self.last_stats = stats;
        self.capture = Some(capture);
        self.read_capture(encoder)
    }

    /// Submits `encoder` plus a copy of the capture target and reads the pixels back.
    pub(crate) fn read_capture(&mut self, mut encoder: wgpu::CommandEncoder) -> Result<ImageData, String> {
        // caller restores self.capture = Some(...) before invoking read_capture.
        let capture = self.capture.as_ref().ok_or_else(|| "capture target was not ensured".to_string())?;
        let size = capture.size;
        let unpadded = size.0 * 4;
        let padded = unpadded.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("spark readback"),
            size: padded as u64 * size.1 as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &capture.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(padded), rows_per_image: Some(size.1) },
            },
            extent(size),
        );
        self.queue.submit([encoder.finish()]);

        let slice = buffer.slice(..);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        self.device.poll(wgpu::PollType::wait_indefinitely()).map_err(|e| format!("GPU wait failed: {e}"))?;
        let data = slice.get_mapped_range().map_err(|e| format!("cannot read screenshot: {e:?}"))?;
        let mut pixels = Vec::with_capacity((unpadded * size.1) as usize);
        for row in 0..size.1 as usize {
            let start = row * padded as usize;
            pixels.extend_from_slice(&data[start..start + unpadded as usize]);
        }
        drop(data);
        buffer.unmap();
        Ok(ImageData::new(size.0, size.1, pixels))
    }

    // ------------------------------------------------------------------------------------------

    // ------------------------------------------------------------------------------------------
}

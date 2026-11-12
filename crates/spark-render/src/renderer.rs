//! The `Renderer` type: construction, window surface, device recovery and the per-frame entry points.

use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use spark_core::{
    ImageData, Mat4, World,
};

use crate::consts::*;
use crate::init::*;
use crate::resources::*;
use crate::stats::*;
use crate::uniforms::*;
/// Draws a [`World`] to a window or to an image.
pub struct Renderer {
    pub(crate) instance: wgpu::Instance,
    pub(crate) device: wgpu::Device,
    pub(crate) queue: wgpu::Queue,
    pub(crate) info: GpuInfo,
    pub(crate) surface: Option<SurfaceState>,
    pub(crate) surface_factory: Option<SurfaceFactory>,
    pub(crate) width: u32,
    pub(crate) height: u32,

    pub(crate) frame_buffer: wgpu::Buffer,
    pub(crate) frame_bind_group: wgpu::BindGroup,
    pub(crate) object_layout: wgpu::BindGroupLayout,
    pub(crate) object_buffer: wgpu::Buffer,
    pub(crate) object_bind_group: wgpu::BindGroup,
    /// Object buffer size in bytes.
    pub(crate) object_capacity: usize,
    /// Dynamic offset alignment of the object buffer.
    pub(crate) object_align: u64,
    pub(crate) frame_layout: wgpu::BindGroupLayout,
    pub(crate) adapter_formats: HashMap<wgpu::TextureFormat, wgpu::TextureFormatFeatureFlags>,
    pub(crate) shadow_sampler: wgpu::Sampler,
    /// 1x1 map bound when shadows are off.
    pub(crate) shadow_dummy: ShadowMap,
    pub(crate) shadow: Option<ShadowMap>,
    /// Trilinear sampler of sky cubemaps.
    pub(crate) sky_sampler: wgpu::Sampler,
    /// 1x1 white cube bound when the sky has no cubemap.
    pub(crate) sky_dummy: GpuCube,
    pub(crate) cubemaps: Vec<GpuCube>,
    /// What `frame_bind_group` was built with: shadow map size (None = dummy), sky cubemap, light shadow face size.
    pub(crate) frame_group_key: (Option<u32>, Option<usize>, Option<u32>),
    /// One-layer map bound when no light casts shadows.
    pub(crate) light_shadow_dummy: LightShadowMap,
    pub(crate) light_shadow: Option<LightShadowMap>,
    pub(crate) light_slots: [Option<LightSlot>; MAX_SHADOWED],
    /// Round robin over the slots (`RenderSettings::light_shadow_updates`).
    pub(crate) light_shadow_cursor: usize,
    /// SHADOW_FACES view-projections, `light_shadow_stride` bytes apart (dynamic offsets).
    pub(crate) light_shadow_buffer: wgpu::Buffer,
    pub(crate) light_shadow_stride: u64,
    pub(crate) light_shadow_bind_group: wgpu::BindGroup,
    pub(crate) light_shadow_pipeline: wgpu::RenderPipeline,
    pub(crate) light_batches: Vec<Batch3>,
    /// Which rendered light (index into this frame's render list) each of `light_batches` belongs to.
    pub(crate) light_batch_owner: Vec<usize>,
    /// Cube faces (bit per face, `cube_face_mask`) each of `light_batches` can reach.
    pub(crate) light_batch_faces: Vec<u8>,
    pub(crate) sky_pipeline_layout: wgpu::PipelineLayout,
    pub(crate) sky_shader: wgpu::ShaderModule,
    /// Skybox pipelines per MSAA sample count.
    pub(crate) sky_pipelines: HashMap<u32, wgpu::RenderPipeline>,
    pub(crate) shadow_buffer: wgpu::Buffer,
    pub(crate) shadow_bind_group: wgpu::BindGroup,
    pub(crate) shadow_pipeline: wgpu::RenderPipeline,
    pub(crate) depth_resolve_layout: wgpu::BindGroupLayout,
    pub(crate) depth_resolve_layout_p: wgpu::PipelineLayout,
    pub(crate) depth_resolve_shader: wgpu::ShaderModule,
    pub(crate) depth_resolve_pipelines: HashMap<u32, wgpu::RenderPipeline>,
    pub(crate) texture_layout: wgpu::BindGroupLayout,
    /// nearest / linear with Repeat (meshes), then nearest / linear with ClampToEdge (2D canvas, post input).
    pub(crate) texture_samplers: [wgpu::Sampler; 4],
    /// Group 3 (surface) / group 1 (post): params, texture1, texture2, linear + nearest samplers.
    pub(crate) shader_layout: wgpu::BindGroupLayout,
    pub(crate) mesh_pipeline_layout: wgpu::PipelineLayout,
    pub(crate) post_layout: wgpu::BindGroupLayout,
    pub(crate) post_pipeline_layout: wgpu::PipelineLayout,
    pub(crate) default_surface: GpuShader,
    pub(crate) copy_post: GpuShader,
    /// Built-in post-processing effect shaders (indexed by PostFxKind).
    pub(crate) built_in_post: Vec<GpuShader>,
    /// ShaderData for built-in post effects (for param value management).
    pub(crate) built_in_post_data: Vec<spark_core::shader::ShaderData>,
    pub(crate) shaders: Vec<GpuShader>,
    pub(crate) post_targets: Vec<PostTarget>,
    pub(crate) pass_buffers: Vec<wgpu::Buffer>,

    pub(crate) canvas_buffer: wgpu::Buffer,
    pub(crate) canvas_bind_group: wgpu::BindGroup,
    pub(crate) canvas_pipeline_layout: wgpu::PipelineLayout,
    pub(crate) canvas_shader: wgpu::ShaderModule,
    pub(crate) canvas_pipelines: HashMap<wgpu::TextureFormat, wgpu::RenderPipeline>,
    pub(crate) canvas_vertices: wgpu::Buffer,
    /// First vertex of the "ui" layer in `canvas_vertices` (the "scene" layer comes first).
    pub(crate) canvas_ui_base: u32,
    pub(crate) glyphs: Option<GpuTexture>,
    pub(crate) glyph_version: u64,

    pub(crate) scene_target: Option<SceneTarget>,
    pub(crate) capture: Option<Capture>,
    pub(crate) meshes: Vec<Option<GpuMesh>>,
    pub(crate) textures: Vec<GpuTexture>,
    pub(crate) white: GpuTexture,
    /// Flat tangent-space normal (bound as `normal_texture` when a material has no normal map).
    pub(crate) flat_normal: wgpu::TextureView,
    /// Texture bind groups of normal-mapped materials: (base texture, normal map, nearest).
    pub(crate) normal_mapped_groups: HashMap<(Option<usize>, usize, bool), wgpu::BindGroup>,
    pub(crate) assets_generation: u64,
    /// `Assets::mesh_edits` already applied.
    pub(crate) mesh_edits: u64,
    pub(crate) candidates: Vec<Candidate>,
    pub(crate) order: Vec<u32>,
    pub(crate) batches: Vec<Batch3>,
    pub(crate) shadow_batches: Vec<Batch3>,
    pub(crate) scratch: Vec<u8>,
    pub(crate) world_matrices: Vec<Mat4>,
    pub(crate) world_visible: Vec<bool>,
    pub(crate) device_lost: Arc<AtomicBool>,
    /// Format of the 3D scene image and intermediate post images: HDR (Rgba16Float) when supported.
    pub(crate) scene_format: wgpu::TextureFormat,
    /// (frame number, view_proj) of the last drawn frame and the view_proj before it (motion blur).
    pub(crate) camera_history: Option<(u64, Mat4)>,
    pub(crate) prev_view_proj: Mat4,
    /// Numbers about the last frame drawn by [`Renderer::render`] (shown by the debug overlay).
    pub(crate) last_stats: FrameStats,
}

impl Renderer {
    /// Renderer that presents to a window.
    pub fn new_windowed<W>(window: Arc<W>, width: u32, height: u32, vsync: bool) -> Result<Self, String>
    where
        W: wgpu::rwh::HasWindowHandle + wgpu::rwh::HasDisplayHandle + std::fmt::Debug + Send + Sync + 'static,
    {
        sanitize_backend_env();
        let instance =
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_with_display_handle_from_env(Box::new(window.clone())));
        let surface = instance.create_surface(window.clone()).map_err(|e| format!("cannot create window surface: {e}"))?;
        let adapter = request_adapter(&instance, Some(&surface))?;
        let (device, queue) = request_device(&adapter)?;

        let caps = surface.get_capabilities(&adapter);
        let Some(&first) = caps.formats.first() else {
            return Err("the GPU cannot present to this window".into());
        };
        let format = caps.formats.iter().copied().find(|f| f.is_srgb()).unwrap_or(first);
        let view_format = format.add_srgb_suffix();
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            color_space: wgpu::SurfaceColorSpace::Auto,
            view_formats: if view_format != format { vec![view_format] } else { vec![] },
            alpha_mode: caps.alpha_modes.first().copied().unwrap_or(wgpu::CompositeAlphaMode::Auto),
            width: width.max(1),
            height: height.max(1),
            desired_maximum_frame_latency: 2,
            present_mode: if vsync { wgpu::PresentMode::AutoVsync } else { wgpu::PresentMode::AutoNoVsync },
        };
        surface.configure(&device, &config);

        let mut renderer = Self::build(instance, &adapter, device, queue, width, height);
        renderer.surface = Some(SurfaceState { surface, config, view_format });
        renderer.surface_factory = Some(Box::new(move |instance| instance.create_surface(window.clone()).ok()));
        Ok(renderer)
    }

    /// Renderer without a window (tests, CI, AI screenshots).
    pub fn new_headless(width: u32, height: u32) -> Result<Self, String> {
        sanitize_backend_env();
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
        let adapter = request_adapter(&instance, None)?;
        let (device, queue) = request_device(&adapter)?;
        Ok(Self::build(instance, &adapter, device, queue, width, height))
    }

    /// Renderer on a GPU device owned by someone else (the editor shares egui's device so the
    /// game is drawn straight into a texture shown in the UI). Draw with [`Self::render_to_view`].
    pub fn from_device(
        instance: wgpu::Instance,
        adapter: &wgpu::Adapter,
        device: wgpu::Device,
        queue: wgpu::Queue,
        width: u32,
        height: u32,
    ) -> Self {
        Self::build(instance, adapter, device, queue, width.max(1), height.max(1))
    }

    /// The GPU device (shared with the caller when built by [`Self::from_device`]).
    pub fn device(&self) -> &wgpu::Device {
        &self.device
    }

    /// Draws the world into any texture view of `format` (sized like [`Self::size`]).
    pub fn render_to_view(
        &mut self,
        world: &World,
        view: &wgpu::TextureView,
        format: wgpu::TextureFormat,
    ) -> Result<FrameStats, String> {
        if self.is_device_lost() {
            return Err("GPU device lost".into());
        }
        self.ensure_canvas_pipeline(format);
        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("spark view") });
        let mut stats = self.draw_scene(&mut encoder, world);
        stats.post_passes = self.draw_post(&mut encoder, view, format, world);
        self.last_stats = stats;
        self.queue.submit([encoder.finish()]);
        Ok(stats)
    }

    pub fn info(&self) -> &GpuInfo {
        &self.info
    }

    /// Output size in pixels.
    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        self.width = width;
        self.height = height;
        if let Some(s) = &mut self.surface {
            s.config.width = width;
            s.config.height = height;
            s.surface.configure(&self.device, &s.config);
        }
    }

    /// Numbers about the last rendered frame.
    pub fn last_stats(&self) -> FrameStats {
        self.last_stats
    }

    /// Whether the GPU device was lost (driver crash, TDR, etc.).
    pub fn is_device_lost(&self) -> bool {
        self.device_lost.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Recreates device, queue and GPU resources after a device loss (the window surface is
    /// reconfigured). Assets are re-uploaded on the next frame.
    pub fn recover(&mut self) -> Result<(), String> {
        log::info!("attempting GPU device recovery...");
        self.device_lost.store(false, std::sync::atomic::Ordering::SeqCst);

        let surface = self.surface.take();
        let surface_factory = self.surface_factory.take();
        let width = self.width;
        let height = self.height;

        // Move the instance out (replace with a dummy that will be discarded on overwrite).
        let dummy = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
        let instance = std::mem::replace(&mut self.instance, dummy);

        let adapter = request_adapter(&instance, surface.as_ref().map(|s| &s.surface))?;
        let (device, queue) = request_device(&adapter)?;

        let mut new = Self::build(instance, &adapter, device, queue, width, height);
        new.surface = surface;
        new.surface_factory = surface_factory;

        if let Some(s) = &new.surface {
            s.surface.configure(&new.device, &s.config);
        }

        *self = new;
        Ok(())
    }

    /// Draws the world to the window.
    pub fn render(&mut self, world: &World) -> Result<FrameStats, String> {
        self.render_frame(world, false).map(|(stats, _)| stats)
    }

    /// Draws the world to the window and also returns that frame as an image (F12): the scene
    /// is drawn once, only the cheap post chain runs a second time into the capture target.
    pub fn render_and_capture(&mut self, world: &World) -> Result<(FrameStats, Option<ImageData>), String> {
        self.render_frame(world, true)
    }

    /// [`Self::render`], plus the frame as an image when `capture` is true.
    pub fn render_and_capture_if(&mut self, world: &World, capture: bool) -> Result<Option<ImageData>, String> {
        self.render_frame(world, capture).map(|(_, image)| image)
    }

    pub(crate) fn render_frame(&mut self, world: &World, capture: bool) -> Result<(FrameStats, Option<ImageData>), String> {
        if self.is_device_lost() {
            return Err("GPU device lost".into());
        }
        let Some(state) = &self.surface else {
            return Err("render(): this renderer is headless, use screenshot()".into());
        };
        let view_format = state.view_format;
        let frame = match state.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(t) => t,
            wgpu::CurrentSurfaceTexture::Suboptimal(t) => {
                drop(t);
                self.reconfigure();
                return Ok((FrameStats::default(), None));
            }
            wgpu::CurrentSurfaceTexture::Outdated => {
                self.reconfigure();
                return Ok((FrameStats::default(), None));
            }
            wgpu::CurrentSurfaceTexture::Lost => {
                self.recreate_surface();
                return Ok((FrameStats::default(), None));
            }
            wgpu::CurrentSurfaceTexture::Validation => return Err("window surface validation error".into()),
            #[allow(unreachable_patterns)]
            _ => return Ok((FrameStats::default(), None)),
        };
        let view = frame.texture.create_view(&wgpu::TextureViewDescriptor { format: Some(view_format), ..Default::default() });
        self.ensure_canvas_pipeline(view_format);
        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("spark frame") });
        let mut stats = self.draw_scene(&mut encoder, world);
        stats.post_passes = self.draw_post(&mut encoder, &view, view_format, world);
        self.last_stats = stats;
        if !capture {
            self.queue.submit([encoder.finish()]);
            self.queue.present(frame);
            return Ok((stats, None));
        }
        self.ensure_capture();
        self.ensure_canvas_pipeline(COLOR_FORMAT);
        // ensure_capture() was just called, so self.capture is Some.
        let target = self.capture.take().ok_or_else(|| "capture target was not ensured".to_string())?;
        self.draw_post(&mut encoder, &target.view, COLOR_FORMAT, world);
        self.capture = Some(target);
        let image = self.read_capture(encoder);
        self.queue.present(frame);
        Ok((stats, Some(image?)))
    }

    pub(crate) fn reconfigure(&mut self) {
        if let Some(s) = &self.surface {
            s.surface.configure(&self.device, &s.config);
        }
    }

    pub(crate) fn recreate_surface(&mut self) {
        if let (Some(factory), Some(state)) = (&self.surface_factory, &mut self.surface) {
            match factory(&self.instance) {
                Some(surface) => {
                    state.surface = surface;
                    state.surface.configure(&self.device, &state.config);
                }
                None => log::error!("window surface lost and could not be recreated"),
            }
        }
    }

}

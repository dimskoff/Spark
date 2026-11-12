//! GPU bring-up: adapter / device selection and building every long-lived GPU resource of the renderer.

use std::collections::HashMap;
use std::num::NonZeroU64;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use spark_core::shader::COPY_POST;
use spark_core::{
    Color, ImageData, Mat4, ShaderData, Vertex,
};
use wgpu::util::DeviceExt;

use crate::assets::*;
use crate::canvas::*;
use crate::consts::*;
use crate::gpu_util::*;
use crate::lighting::*;
use crate::renderer::*;
use crate::shaders::*;
use crate::stats::*;
use crate::uniforms::*;

impl Renderer {
    pub(crate) fn build(
        instance: wgpu::Instance,
        adapter: &wgpu::Adapter,
        device: wgpu::Device,
        queue: wgpu::Queue,
        width: u32,
        height: u32,
    ) -> Self {
        let ai = adapter.get_info();
        let info = GpuInfo {
            name: ai.name.clone(),
            backend: format!("{:?}", ai.backend),
            device_type: format!("{:?}", ai.device_type),
            driver: format!("{} {}", ai.driver, ai.driver_info).trim().to_string(),
        };

        // GPU error handling: log uncaptured errors instead of panicking.
        device.on_uncaptured_error(Arc::new(|e| {
            match e {
                wgpu::Error::Validation { description, .. } => log::error!("GPU validation error: {description}"),
                wgpu::Error::OutOfMemory { source, .. } => log::error!("GPU out of memory: {source}"),
                wgpu::Error::Internal { description, .. } => log::error!("GPU internal error: {description}"),
            }
        }));

        // Device loss tracking: set a flag the renderer can check before drawing.
        let device_lost = Arc::new(AtomicBool::new(false));
        device.set_device_lost_callback({
            let flag = device_lost.clone();
            Box::new(move |reason, msg| {
                log::error!("GPU device lost: {reason:?}: {msg}");
                flag.store(true, std::sync::atomic::Ordering::SeqCst);
            })
        });

        // HDR scene (values above 1 survive until tonemapping / bloom) where the GPU can render,
        // blend and filter half floats; 8-bit sRGB otherwise (some GLES devices).
        let hdr = adapter.get_texture_format_features(HDR_FORMAT);
        let hdr_ok = hdr.allowed_usages.contains(wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING)
            && hdr.flags.contains(wgpu::TextureFormatFeatureFlags::FILTERABLE | wgpu::TextureFormatFeatureFlags::BLENDABLE);
        let scene_format = if hdr_ok { HDR_FORMAT } else { COLOR_FORMAT };
        // Without TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES on the device only the spec's 1x / 4x are valid,
        // whatever the adapter reports (2x on NVIDIA / Vulkan: texture creation fails, then a panic).
        let specific = device.features().contains(wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES);
        let adapter_formats = [COLOR_FORMAT, DEPTH_FORMAT, scene_format]
            .into_iter()
            .map(|f| {
                let mut flags = adapter.get_texture_format_features(f).flags;
                if !specific {
                    flags.remove(
                        wgpu::TextureFormatFeatureFlags::MULTISAMPLE_X2
                            | wgpu::TextureFormatFeatureFlags::MULTISAMPLE_X8
                            | wgpu::TextureFormatFeatureFlags::MULTISAMPLE_X16,
                    );
                }
                (f, flags)
            })
            .collect::<HashMap<_, _>>();
        let frame_size = size_of::<FrameUniform>() as u64;
        let object_size = size_of::<ObjectUniform>() as u64;
        let vf = wgpu::ShaderStages::VERTEX_FRAGMENT;
        let fs = wgpu::ShaderStages::FRAGMENT;

        let depth_entry = |binding: u32, multisampled: bool, visibility: wgpu::ShaderStages| wgpu::BindGroupLayoutEntry {
            binding,
            visibility,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Depth,
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled,
            },
            count: None,
        };
        let frame_layout = bind_group_layout(
            &device,
            "spark frame",
            &[
                uniform_entry(0, false, frame_size, vf),
                depth_entry(1, false, vf),
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: vf,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Comparison),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: fs,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::Cube,
                        multisampled: false,
                    },
                    count: None,
                },
                sampler_entry(4, fs),
                wgpu::BindGroupLayoutEntry {
                    binding: 5,
                    visibility: vf,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension: wgpu::TextureViewDimension::D2Array,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        );
        let object_layout = bind_group_layout(
            &device,
            "spark objects",
            &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: vf,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: true,
                    min_binding_size: NonZeroU64::new(object_size * MAX_INSTANCES as u64),
                },
                count: None,
            }],
        );
        // Base texture, its sampler and the material normal map (2D canvas uses only the first two).
        let texture_layout = bind_group_layout(&device, "spark texture", &[texture_entry(0, fs), sampler_entry(1, fs), texture_entry(2, fs), texture_entry(3, fs)]);
        let shader_layout = bind_group_layout(
            &device,
            "spark shader resources",
            &[uniform_entry(0, false, 0, vf), texture_entry(1, vf), texture_entry(2, vf), sampler_entry(3, vf), sampler_entry(4, vf)],
        );
        let post_layout = bind_group_layout(
            &device,
            "spark post",
            &[
                uniform_entry(0, false, frame_size, vf),
                texture_entry(1, fs),
                sampler_entry(2, fs),
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: fs,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                uniform_entry(4, false, size_of::<PassUniform>() as u64, fs),
            ],
        );

        let frame_buffer = uniform_buffer(&device, "spark frame", frame_size);
        let shadow_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("spark shadow sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            compare: Some(wgpu::CompareFunction::LessEqual),
            ..Default::default()
        });
        let shadow_dummy = shadow_map(&device, 1);
        let light_shadow_dummy = light_shadow_map(&device, 1, 1);
        let sky_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("spark sky sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });
        let sky_dummy = upload_cube(&device, &queue, &std::array::from_fn(|_| ImageData::solid(Color::WHITE)));
        let frame_bind_group =
            frame_bind_group(&device, &frame_layout, &frame_buffer, &shadow_dummy.view, &shadow_sampler, &sky_dummy.view, &sky_sampler, &light_shadow_dummy.view);
        let sky_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("spark sky"),
            bind_group_layouts: &[Some(&frame_layout)],
            immediate_size: 0,
        });
        let sky_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("spark sky"),
            source: wgpu::ShaderSource::Wgsl(spark_core::shader::skybox_source().into()),
        });

        // Per-object uniforms, read in 64-object windows (dynamic offsets) by instanced draws.
        let object_align = device.limits().min_uniform_buffer_offset_alignment as u64;
        let object_capacity = 64 * 1024;
        let (object_buffer, object_bind_group) = object_buffer(&device, &object_layout, object_capacity);

        let shadow_layout =
            bind_group_layout(&device, "spark shadow", &[uniform_entry(0, false, 64, wgpu::ShaderStages::VERTEX)]);
        let shadow_buffer = uniform_buffer(&device, "spark shadow view", 64);
        let shadow_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("spark shadow"),
            layout: &shadow_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: shadow_buffer.as_entire_binding() }],
        });
        let shadow_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("spark shadow"),
            bind_group_layouts: &[Some(&shadow_layout), Some(&object_layout)],
            immediate_size: 0,
        });
        let shadow_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("spark shadow shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/shadow.wgsl").into()),
        });
        // Depth-only pipeline shared by the sun map and the light cubes (they differ in the view binding).
        let depth_only = |label: &str, layout: &wgpu::PipelineLayout| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(layout),
                vertex: wgpu::VertexState {
                    module: &shadow_shader,
                    entry_point: Some("vs_main"),
                    compilation_options: Default::default(),
                    buffers: &[Some(wgpu::VertexBufferLayout {
                        array_stride: size_of::<Vertex>() as u64,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &VERTEX_ATTRIBUTES[..1],
                    })],
                },
                primitive: wgpu::PrimitiveState { cull_mode: None, ..Default::default() },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: SHADOW_FORMAT,
                    depth_write_enabled: Some(true),
                    depth_compare: Some(wgpu::CompareFunction::LessEqual),
                    stencil: Default::default(),
                    bias: wgpu::DepthBiasState { constant: 2, slope_scale: 2.5, clamp: 0.0 },
                }),
                multisample: Default::default(),
                fragment: None,
                multiview_mask: None,
                cache: None,
            })
        };
        let shadow_pipeline = depth_only("spark shadow", &shadow_pipeline_layout);
        // Light cubes: every face's view-projection lives in one buffer, picked with a dynamic offset.
        let light_shadow_layout =
            bind_group_layout(&device, "spark light shadow", &[uniform_entry(0, true, 64, wgpu::ShaderStages::VERTEX)]);
        let light_shadow_stride = object_align.max(64);
        let light_shadow_buffer = uniform_buffer(&device, "spark light shadow views", light_shadow_stride * SHADOW_FACES as u64);
        let light_shadow_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("spark light shadow"),
            layout: &light_shadow_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &light_shadow_buffer,
                    offset: 0,
                    size: NonZeroU64::new(64),
                }),
            }],
        });
        let light_shadow_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("spark light shadow"),
            bind_group_layouts: &[Some(&light_shadow_layout), Some(&object_layout)],
            immediate_size: 0,
        });
        let light_shadow_pipeline = depth_only("spark light shadow", &light_shadow_pipeline_layout);
        let depth_resolve_layout = bind_group_layout(&device, "spark depth resolve", &[depth_entry(0, true, fs)]);
        let depth_resolve_layout_p = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("spark depth resolve"),
            bind_group_layouts: &[Some(&depth_resolve_layout)],
            immediate_size: 0,
        });
        let depth_resolve_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("spark depth resolve"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/depth_resolve.wgsl").into()),
        });

        let sampler = |filter: wgpu::FilterMode, address: wgpu::AddressMode| {
            device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("spark sampler"),
                address_mode_u: address,
                address_mode_v: address,
                address_mode_w: address,
                mag_filter: filter,
                min_filter: filter,
                mipmap_filter: wgpu::MipmapFilterMode::Linear,
                ..Default::default()
            })
        };
        use wgpu::{AddressMode, FilterMode};
        let texture_samplers = [
            sampler(FilterMode::Nearest, AddressMode::Repeat),
            sampler(FilterMode::Linear, AddressMode::Repeat),
            sampler(FilterMode::Nearest, AddressMode::ClampToEdge),
            sampler(FilterMode::Linear, AddressMode::ClampToEdge),
        ];

        let mesh_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("spark mesh"),
            bind_group_layouts: &[Some(&frame_layout), Some(&object_layout), Some(&texture_layout), Some(&shader_layout)],
            immediate_size: 0,
        });
        let post_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("spark post"),
            bind_group_layouts: &[Some(&post_layout), Some(&shader_layout)],
            immediate_size: 0,
        });
        // (128, 128, 255) = +Z in tangent space; read back as data by `surface_normal`.
        let flat_normal = device
            .create_texture_with_data(
                &queue,
                &wgpu::TextureDescriptor {
                    label: Some("spark flat normal"),
                    size: extent((1, 1)),
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: COLOR_FORMAT,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                    view_formats: &[],
                },
                wgpu::util::TextureDataOrder::LayerMajor,
                &[128, 128, 255, 255],
            )
            .create_view(&Default::default());
        // (255, 255, 255): no occlusion, roughness / metallic factors unchanged.
        let flat_orm = device
            .create_texture_with_data(
                &queue,
                &wgpu::TextureDescriptor {
                    label: Some("spark flat orm"),
                    size: extent((1, 1)),
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: COLOR_FORMAT,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                    view_formats: &[],
                },
                wgpu::util::TextureDataOrder::LayerMajor,
                &[255, 255, 255, 255],
            )
            .create_view(&Default::default());
        let flat_normal = crate::resources::FlatMaps { normal: flat_normal, orm: flat_orm };
        let white = upload_texture(
            &device,
            &queue,
            &texture_layout,
            &texture_samplers,
            &flat_normal,
            &ImageData::solid(Color::WHITE),
            COLOR_FORMAT,
            false,
        );

        let builtin = |name: &str, code: &str| {
            // built-in shaders are known-good at compile time.
            let data = ShaderData::compile(name, code).expect("built-in shader compiles");
            gpu_shader(&device, &shader_layout, &texture_samplers, &white, &[], &data)
        };
        let default_surface = builtin("spark default surface", "");
        let copy_post = builtin("spark upscale", COPY_POST);

        // Compile all built-in post-processing effect shaders.
        let mut built_in_post = Vec::with_capacity(spark_core::shader::PostFxKind::COUNT);
        let mut built_in_post_data = Vec::with_capacity(spark_core::shader::PostFxKind::COUNT);
        for kind in spark_core::shader::PostFxKind::ALL {
            let name = kind.name();
            let code = kind.shader_source();
            // Validated by spark-core's `builtin_post_effects_compile` test.
            let data = ShaderData::compile(name, code).unwrap_or_else(|e| panic!("built-in post shader '{name}': {e}"));
            let gpu = gpu_shader(&device, &shader_layout, &texture_samplers, &white, &[], &data);
            built_in_post.push(gpu);
            built_in_post_data.push(data);
        }

        let canvas_layout = bind_group_layout(&device, "spark canvas", &[uniform_entry(0, false, 16, wgpu::ShaderStages::VERTEX)]);
        let canvas_buffer = uniform_buffer(&device, "spark canvas", 16);
        let canvas_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("spark canvas"),
            layout: &canvas_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: canvas_buffer.as_entire_binding() }],
        });
        let canvas_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("spark canvas"),
            bind_group_layouts: &[Some(&canvas_layout), Some(&texture_layout)],
            immediate_size: 0,
        });
        let canvas_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("spark canvas shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/canvas.wgsl").into()),
        });
        let mut canvas_pipelines = HashMap::new();
        for f in [COLOR_FORMAT, scene_format] {
            canvas_pipelines.entry(f).or_insert_with(|| canvas_pipeline(&device, &canvas_pipeline_layout, &canvas_shader, f));
        }
        let canvas_vertices = vertex_buffer(&device, 64 * 1024);

        Self {
            instance,
            device,
            queue,
            info,
            surface: None,
            surface_factory: None,
            width: width.max(1),
            height: height.max(1),
            frame_buffer,
            frame_bind_group,
            object_layout,
            object_buffer,
            object_bind_group,
            object_capacity,
            object_align,
            frame_layout,
            adapter_formats,
            shadow_sampler,
            shadow_dummy,
            shadow: None,
            sky_sampler,
            sky_dummy,
            cubemaps: Vec::new(),
            frame_group_key: (None, None, None),
            light_shadow_dummy,
            light_shadow: None,
            light_slots: [None; MAX_SHADOWED],
            light_shadow_cursor: 0,
            light_shadow_buffer,
            light_shadow_stride,
            light_shadow_bind_group,
            light_shadow_pipeline,
            light_batches: Vec::new(),
            light_batch_owner: Vec::new(),
            light_batch_faces: Vec::new(),
            sky_pipeline_layout,
            sky_shader,
            sky_pipelines: HashMap::new(),
            shadow_buffer,
            shadow_bind_group,
            shadow_pipeline,
            depth_resolve_layout,
            depth_resolve_layout_p,
            depth_resolve_shader,
            depth_resolve_pipelines: HashMap::new(),
            texture_layout,
            texture_samplers,
            shader_layout,
            mesh_pipeline_layout,
            post_layout,
            post_pipeline_layout,
            default_surface,
            copy_post,
            built_in_post,
            built_in_post_data,
            shaders: Vec::new(),
            post_targets: Vec::new(),
            pass_buffers: Vec::new(),
            canvas_buffer,
            canvas_bind_group,
            canvas_pipeline_layout,
            canvas_shader,
            canvas_pipelines,
            canvas_vertices,
            canvas_ui_base: 0,
            glyphs: None,
            glyph_version: 0,
            scene_target: None,
            capture: None,
            meshes: Vec::new(),
            textures: Vec::new(),
            white,
            flat_normal,
            normal_mapped_groups: HashMap::new(),
            assets_generation: u64::MAX,
            mesh_edits: 0,
            candidates: Vec::new(),
            order: Vec::new(),
            batches: Vec::new(),
            shadow_batches: Vec::new(),
            scratch: Vec::new(),
            world_matrices: Vec::new(),
            world_visible: Vec::new(),
            device_lost,
            scene_format,
            camera_history: None,
            prev_view_proj: Mat4::IDENTITY,
            last_stats: FrameStats::default(),
        }
    }
}

pub(crate) fn request_adapter(instance: &wgpu::Instance, surface: Option<&wgpu::Surface<'_>>) -> Result<wgpu::Adapter, String> {
    let mut tried: Vec<&str> = Vec::new();
    let mut errors: Vec<String> = Vec::new();

    for (pref, fallback, label) in [
        (wgpu::PowerPreference::HighPerformance, false, "HighPerformance"),
        (wgpu::PowerPreference::LowPower, false, "LowPower"),
        (wgpu::PowerPreference::None, true, "force_fallback_adapter"),
    ] {
        tried.push(label);
        match pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: pref,
            force_fallback_adapter: fallback,
            compatible_surface: surface,
            ..Default::default()
        })) {
            Ok(adapter) => {
                let info = adapter.get_info();
                log::info!("GPU adapter selected via {label}: {} ({:?})", info.name, info.backend);
                return Ok(adapter);
            }
            Err(e) => {
                log::warn!("request_adapter {label} failed: {e}");
                errors.push(format!("{label}: {e}"));
            }
        }
    }

    let available: Vec<String> = pollster::block_on(instance.enumerate_adapters(wgpu::Backends::all()))
        .iter()
        .map(|a| {
            let info = a.get_info();
            format!("{} ({:?}, {:?})", info.name, info.backend, info.device_type)
        })
        .collect();

    Err(format!(
        "no compatible GPU found.\n\
         Tried: {}\n\
         Errors: {}\n\
         Available GPUs: {}\n\
         Hint: install up-to-date GPU drivers, or try WGPU_BACKEND=gl for the OpenGL backend.",
        tried.join(", "),
        errors.join("; "),
        if available.is_empty() { "none".into() } else { available.join(", ") },
    ))
}

pub(crate) fn request_device(adapter: &wgpu::Adapter) -> Result<(wgpu::Device, wgpu::Queue), String> {
    // Ask only for WebGL2-level limits so weak / old GPUs (and the GL backend) work.
    let desc = wgpu::DeviceDescriptor {
        label: Some("spark"),
        // lets MSAA use every sample count the GPU has (2x / 8x), not only the spec's 4x
        required_features: adapter.features() & wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES,
        required_limits: wgpu::Limits::downlevel_webgl2_defaults().using_resolution(adapter.limits()),
        memory_hints: wgpu::MemoryHints::Performance,
        ..Default::default()
    };
    pollster::block_on(adapter.request_device(&desc)).map_err(|e| format!("cannot open GPU device: {e}"))
}

pub(crate) fn sanitize_backend_env() {
    if std::env::var_os("WGPU_BACKEND").is_some_and(|v| v.to_string_lossy().trim().is_empty()) {
        // SAFETY: called before the GPU instance (and any of its threads) exists.
        unsafe { std::env::remove_var("WGPU_BACKEND") };
    }
}

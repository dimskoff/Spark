//! Spark Engine - Rendering: wgpu backend (DX12 / Vulkan / Metal / OpenGL), custom WGSL shaders, screenshots.
//!
//! The renderer is a *view* of [`World`]: it uploads new assets automatically and draws the scene.
//! Games never touch GPU objects. Force a backend with the `WGPU_BACKEND` env var (`dx12`, `vulkan`, `gl`).
//!
//! Frame = scene pass at the internal resolution (surface shaders) -> 2D canvas "scene" layer -> post
//! passes (game shaders, or a plain upscale) -> 2D canvas "ui" layer -> window/screenshot.
//! The engine has no looks of its own; see `spark_core::shader`.
//!
//! Module map:
//! - `renderer`     - the [`Renderer`] type, window surface, device recovery, frame entry points
//! - `init`         - adapter / device selection, building the long-lived GPU resources
//! - `assets`       - mesh / texture / cubemap upload
//! - `scene`        - 3D pass: culling, batching, shadow passes, main color pass
//! - `lighting`     - light collection, sun and point-light shadow maps
//! - `scene_target` - scene color / depth targets, MSAA, sky pipeline
//! - `post`         - post chain and the final 2D "ui" layer
//! - `canvas`       - 2D canvas drawing
//! - `shaders`      - game / built-in shader modules and pipelines
//! - `capture`      - screenshots and frame readback
//! - `uniforms`, `consts`, `resources`, `stats`, `gpu_util`, `culling` - shared types and helpers

mod assets;
mod canvas;
mod capture;
mod consts;
mod culling;
mod gpu_util;
mod init;
mod lighting;
mod post;
mod renderer;
mod resources;
mod scene;
mod scene_target;
mod shaders;
mod stats;
mod uniforms;

#[cfg(test)]
mod tests;

pub use renderer::Renderer;
pub use stats::{FrameStats, GpuInfo};
pub use wgpu;

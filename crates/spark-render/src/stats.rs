//! Public information types: GPU description and per-frame statistics.

/// GPU / driver info.
#[derive(Clone, Debug)]
pub struct GpuInfo {
    pub name: String,
    pub backend: String,
    pub device_type: String,
    pub driver: String,
}

impl std::fmt::Display for GpuInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({}, {}, driver: {})", self.name, self.backend, self.device_type, self.driver)
    }
}

/// Numbers about the last rendered frame.
#[derive(Clone, Copy, Debug, Default)]
pub struct FrameStats {
    pub draw_calls: u32,
    /// Objects skipped because they were outside the camera view.
    pub culled: u32,
    pub triangles: u64,
    pub internal_size: (u32, u32),
    /// Post passes run (0 = plain upscale).
    pub post_passes: u32,
}

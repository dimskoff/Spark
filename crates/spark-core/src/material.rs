use glam::Vec2;
use spark_reflect::{Reflect, ReflectEnum};

use crate::assets::TextureId;
use crate::color::Color;
use crate::shader::ShaderId;

/// How a material is combined with what is behind it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, ReflectEnum)]
pub enum BlendMode {
    /// `Alpha` when `color.a < 1`, otherwise `Opaque`.
    #[default]
    #[reflect(name = "auto")]
    Auto,
    /// Solid; pixels with alpha < 0.01 are cut out (foliage, fences).
    #[reflect(name = "opaque")]
    Opaque,
    /// See-through (glass, water, fading objects). Drawn after opaque objects, back to front.
    #[reflect(name = "alpha")]
    Alpha,
    /// Adds light (fire, lasers, glow). Drawn with the transparent objects.
    #[reflect(name = "additive")]
    Additive,
}

impl BlendMode {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "auto" => Some(Self::Auto),
            "opaque" => Some(Self::Opaque),
            "alpha" | "transparent" => Some(Self::Alpha),
            "additive" | "add" => Some(Self::Additive),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Opaque => "opaque",
            Self::Alpha => "alpha",
            Self::Additive => "additive",
        }
    }
}

/// How a mesh looks. Lit by the sun + ambient unless `unlit`.
#[derive(Clone, Copy, Debug, PartialEq, Reflect)]
pub struct Material {
    /// Base color (multiplied with the texture).
    #[reflect(lua = "color")]
    pub color: Color,
    /// Base color texture (a runtime handle; no scene-file form until assets have paths).
    #[reflect(readonly, transient)]
    pub texture: Option<TextureId>,
    /// Texture repeats across the surface (e.g. `(4, 4)` on a ground plane).
    pub tiling: Vec2,
    /// Ignore lighting: shows the exact color (UI, glowing things, sprites).
    pub unlit: bool,
    /// Surface shader (`None` = [`crate::RenderSettings::shader`], else the engine default).
    #[reflect(readonly, transient, lua = "shader")]
    pub shader: Option<ShaderId>,
    /// Free per-material numbers for custom shaders (`object.data` in WGSL).
    #[reflect(lua = "data")]
    pub data: [f32; 4],
    /// Blending with the background (see [`BlendMode`]).
    #[reflect(lua = "blend")]
    pub blend: BlendMode,
    /// Draw back faces too (leaves, flags, planes seen from below).
    #[reflect(lua = "double_sided")]
    pub double_sided: bool,
    /// Casts a shadow from the sun (`RenderSettings::shadows`).
    #[reflect(lua = "cast_shadow")]
    pub cast_shadow: bool,
    /// Physically based shading (Cook-Torrance GGX with `metallic` / `roughness`) instead of the
    /// classic diffuse look. Turned on by [`Material::pbr`], `with_metallic`, `with_roughness`.
    #[reflect(lua = "pbr")]
    pub pbr: bool,
    /// 0 = dielectric (plastic, wood, stone), 1 = metal. Used when `pbr` is on. Out-of-range values are clamped
    /// (like [`Material::with_metallic`], which also turns `pbr` on; writing the field alone does not).
    #[reflect(range = 0.0..=1.0, clamp, step = 0.01, lua = "metallic")]
    pub metallic: f32,
    /// 0 = mirror-smooth, 1 = fully rough. Used when `pbr` is on. Out-of-range values are clamped
    /// (like [`Material::with_roughness`], which also turns `pbr` on; writing the field alone does not).
    #[reflect(range = 0.0..=1.0, clamp, step = 0.01, lua = "roughness")]
    pub roughness: f32,
    /// Light the surface emits by itself, added after lighting (black = none). Works in every mode.
    #[reflect(lua = "emissive")]
    pub emissive: Color,
    /// Tangent-space normal map (OpenGL convention: green points along +V). Works with and without `pbr`.
    #[reflect(readonly, transient, lua = "normal_map")]
    pub normal_map: Option<TextureId>,
    /// Packed surface data (used when `pbr` is on): R = ambient occlusion, G = roughness, B = metallic; each is
    /// multiplied with `roughness` / `metallic`. Like the normal map it holds data, not colors.
    #[reflect(readonly, transient, lua = "orm_map")]
    pub orm_map: Option<TextureId>,
}

impl Default for Material {
    fn default() -> Self {
        Self {
            color: Color::WHITE,
            texture: None,
            tiling: Vec2::ONE,
            unlit: false,
            shader: None,
            data: [0.0; 4],
            blend: BlendMode::Auto,
            double_sided: false,
            cast_shadow: true,
            pbr: false,
            metallic: 0.0,
            roughness: 0.5,
            emissive: Color::BLACK,
            normal_map: None,
            orm_map: None,
        }
    }
}

impl Material {
    /// Lit, solid color.
    pub fn color(color: Color) -> Self {
        Self { color, ..Default::default() }
    }

    /// Unlit, solid color.
    pub fn unlit(color: Color) -> Self {
        Self { color, unlit: true, ..Default::default() }
    }

    /// Lit, textured.
    pub fn textured(texture: TextureId) -> Self {
        Self { texture: Some(texture), ..Default::default() }
    }

    pub fn with_color(mut self, color: Color) -> Self {
        self.color = color;
        self
    }

    pub fn with_texture(mut self, texture: TextureId) -> Self {
        self.texture = Some(texture);
        self
    }

    pub fn with_tiling(mut self, x: f32, y: f32) -> Self {
        self.tiling = Vec2::new(x, y);
        self
    }

    pub fn with_unlit(mut self, unlit: bool) -> Self {
        self.unlit = unlit;
        self
    }

    pub fn with_shader(mut self, shader: Option<ShaderId>) -> Self {
        self.shader = shader;
        self
    }

    pub fn with_blend(mut self, blend: BlendMode) -> Self {
        self.blend = blend;
        self
    }

    pub fn with_double_sided(mut self, on: bool) -> Self {
        self.double_sided = on;
        self
    }

    /// The blend mode actually used (`Auto` resolved).
    pub fn effective_blend(&self) -> BlendMode {
        match self.blend {
            BlendMode::Auto if self.color.a < 0.999 => BlendMode::Alpha,
            BlendMode::Auto => BlendMode::Opaque,
            b => b,
        }
    }

    /// Drawn in the transparent pass (no depth writes, sorted back to front).
    pub fn is_transparent(&self) -> bool {
        matches!(self.effective_blend(), BlendMode::Alpha | BlendMode::Additive)
    }

    pub fn with_data(mut self, data: [f32; 4]) -> Self {
        self.data = data;
        self
    }

    /// PBR material: base `color`, `metallic` (0..1) and `roughness` (0..1).
    pub fn pbr(color: Color, metallic: f32, roughness: f32) -> Self {
        Self::color(color).with_metallic(metallic).with_roughness(roughness)
    }

    /// Sets `metallic` (clamped to 0..1) and turns on PBR shading.
    pub fn with_metallic(mut self, metallic: f32) -> Self {
        self.metallic = unit(metallic);
        self.pbr = true;
        self
    }

    /// Sets `roughness` (clamped to 0..1) and turns on PBR shading.
    pub fn with_roughness(mut self, roughness: f32) -> Self {
        self.roughness = unit(roughness);
        self.pbr = true;
        self
    }

    pub fn with_emissive(mut self, emissive: Color) -> Self {
        self.emissive = emissive;
        self
    }

    pub fn with_orm_map(mut self, orm_map: Option<TextureId>) -> Self {
        self.orm_map = orm_map;
        self
    }

    pub fn with_normal_map(mut self, normal_map: Option<TextureId>) -> Self {
        self.normal_map = normal_map;
        self
    }
}

/// Clamps to 0..1; NaN becomes 0 so it never reaches the shader.
fn unit(v: f32) -> f32 {
    if v.is_nan() { 0.0 } else { v.clamp(0.0, 1.0) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn effective_blend_auto_alpha() {
        let m = Material::color(Color::rgba(1.0, 1.0, 1.0, 0.5));
        assert_eq!(m.effective_blend(), BlendMode::Alpha);
    }

    #[test]
    fn effective_blend_auto_opaque() {
        let m = Material::color(Color::WHITE);
        assert_eq!(m.effective_blend(), BlendMode::Opaque);
    }

    #[test]
    fn blend_parse_roundtrip() {
        for &mode in &[BlendMode::Auto, BlendMode::Opaque, BlendMode::Alpha, BlendMode::Additive] {
            assert_eq!(BlendMode::parse(mode.name()), Some(mode));
        }
    }

    #[test]
    fn material_builder_chain() {
        let m = Material::color(Color::RED).with_blend(BlendMode::Additive).with_double_sided(true).with_tiling(2.0, 3.0);
        assert_eq!(m.blend, BlendMode::Additive);
        assert!(m.double_sided);
        assert_eq!(m.tiling, Vec2::new(2.0, 3.0));
    }

    #[test]
    fn pbr_is_opt_in() {
        let m = Material::default();
        assert!(!m.pbr, "existing games keep the classic look");
        assert_eq!((m.metallic, m.roughness, m.emissive, m.normal_map), (0.0, 0.5, Color::BLACK, None));
    }

    #[test]
    fn pbr_builders_clamp_and_enable() {
        let m = Material::pbr(Color::RED, 2.0, -1.0);
        assert!(m.pbr);
        assert_eq!((m.metallic, m.roughness), (1.0, 0.0));
        assert_eq!(m.color, Color::RED);
        assert!(Material::default().with_roughness(0.5).pbr, "even the default value turns PBR on");
        let n = Material::pbr(Color::RED, f32::NAN, f32::NAN);
        assert_eq!((n.metallic, n.roughness), (0.0, 0.0), "NaN never reaches the shader");
        let e = Material::default().with_emissive(Color::BLUE);
        assert!(!e.pbr, "emissive works without PBR");
    }
}

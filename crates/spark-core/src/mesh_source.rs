//! Mesh recipes: how a mesh was made, so a scene file can say `{ primitive = "box", size = [1, 2, 3] }` or
//! `{ asset = "assets/door.glb", node = "Frame" }` instead of storing vertices (ADR 0003, rule 6).
//!
//! [`Assets`] remembers one [`MeshSource`] per `MeshId`. Meshes it cannot reproduce (`Mesh.custom`, in-place
//! edits made by user code) are [`MeshSource::RuntimeOnly`]: a scene save must report them, never drop them.

use glam::Vec3;
use spark_reflect::ReflectEnum;

use crate::assets::{Assets, MeshId};
use crate::mesh::{MeshData, SurfaceOpts};
use crate::reflect::GREATER_THAN_ZERO;

/// Where a mesh comes from.
#[derive(Clone, Debug, Default, PartialEq, ReflectEnum)]
pub enum MeshSource {
    /// Made at run time and not reproducible from a few numbers (`Mesh.custom`). Cannot be saved.
    #[default]
    #[reflect(name = "runtime")]
    RuntimeOnly,
    /// Cube centered at the origin.
    #[reflect(name = "cube")]
    Cube {
        /// Edge length.
        #[reflect(range = GREATER_THAN_ZERO..=f32::MAX, unit = "m", default = 1.0)]
        size: f32,
    },
    /// Box centered at the origin.
    #[reflect(name = "box")]
    Box {
        /// Full size (width, height, depth).
        #[reflect(default = Vec3::ONE)]
        size: Vec3,
        /// Meters per texture repeat (see [`SurfaceOpts::tile`]).
        tile: Option<f32>,
        /// Face cell size in meters (see [`SurfaceOpts::cell`]).
        cell: Option<f32>,
        /// Built with surface options (a `Mesh.box` options table, even an empty one): faces are laid out as grids,
        /// which orders the vertices differently from the plain box.
        surface: bool,
    },
    /// Flat plane on the XZ axes facing +Y.
    #[reflect(name = "plane")]
    Plane {
        /// Size along X.
        #[reflect(range = GREATER_THAN_ZERO..=f32::MAX, unit = "m", default = 10.0)]
        width: f32,
        /// Size along Z.
        #[reflect(range = GREATER_THAN_ZERO..=f32::MAX, unit = "m", default = 10.0)]
        depth: f32,
        /// Meters per texture repeat.
        tile: Option<f32>,
        /// Face cell size in meters.
        cell: Option<f32>,
        /// Built with surface options (see `box`).
        surface: bool,
    },
    /// Flat quad on the XY axes facing +Z.
    #[reflect(name = "quad")]
    Quad {
        /// Size along X.
        #[reflect(range = GREATER_THAN_ZERO..=f32::MAX, unit = "m", default = 1.0)]
        width: f32,
        /// Size along Y.
        #[reflect(range = GREATER_THAN_ZERO..=f32::MAX, unit = "m", default = 1.0)]
        height: f32,
        /// Meters per texture repeat.
        tile: Option<f32>,
        /// Face cell size in meters.
        cell: Option<f32>,
        /// Built with surface options (see `box`).
        surface: bool,
    },
    /// UV sphere centered at the origin.
    #[reflect(name = "sphere")]
    Sphere {
        /// Radius.
        #[reflect(range = GREATER_THAN_ZERO..=f32::MAX, unit = "m", default = 0.5)]
        radius: f32,
        /// Segments around the equator.
        #[reflect(range = 3..=256, default = 24)]
        segments: u32,
    },
    /// Cylinder along Y, centered at the origin.
    #[reflect(name = "cylinder")]
    Cylinder {
        /// Radius.
        #[reflect(range = GREATER_THAN_ZERO..=f32::MAX, unit = "m", default = 0.5)]
        radius: f32,
        /// Height.
        #[reflect(range = GREATER_THAN_ZERO..=f32::MAX, unit = "m", default = 1.0)]
        height: f32,
        /// Segments around the axis.
        #[reflect(range = 3..=256, default = 24)]
        segments: u32,
    },
    /// Flat-shaded triangle list (`Mesh.triangles`): 3 points per triangle, counter-clockwise.
    #[reflect(name = "triangles")]
    Triangles {
        /// The corner points.
        points: Vec<Vec3>,
    },
    /// A mesh of a loaded model (glTF / FBX): the primitive `primitive` of the node called `node`.
    #[reflect(name = "asset")]
    Asset {
        /// Project-relative path with forward slashes.
        #[reflect(asset = "model")]
        asset: String,
        /// Node name inside the model.
        node: String,
        /// Which primitive of the node's mesh (a node mesh can have several materials).
        primitive: u32,
    },
}

impl MeshSource {
    /// Primitives are pure functions of their numbers.
    pub fn is_primitive(&self) -> bool {
        !matches!(self, Self::RuntimeOnly | Self::Asset { .. })
    }

    /// Rebuilds the mesh from the recipe. Primitives are generated; `Asset` loads the model (once) and copies the
    /// node's mesh. `RuntimeOnly` has no recipe.
    pub fn build(&self, assets: &mut Assets) -> Result<MeshData, String> {
        if let Self::Asset { .. } = self {
            let id = self.resolve(assets)?;
            return assets.mesh(id).cloned().ok_or_else(|| "model mesh vanished".to_string());
        }
        self.build_primitive().ok_or_else(|| "a runtime-only mesh has no recipe".to_string())
    }

    /// The generated mesh of a primitive recipe; `None` for `RuntimeOnly` and `Asset`.
    pub fn build_primitive(&self) -> Option<MeshData> {
        let opts = |tile, cell| SurfaceOpts { tile, cell };
        Some(match self {
            Self::Cube { size } => MeshData::cube(*size),
            Self::Box { size, tile, cell, surface } => {
                if *surface {
                    MeshData::cuboid_with(size.x, size.y, size.z, opts(*tile, *cell))
                } else {
                    MeshData::cuboid(size.x, size.y, size.z)
                }
            }
            Self::Plane { width, depth, tile, cell, surface } => {
                if *surface {
                    MeshData::plane_with(*width, *depth, opts(*tile, *cell))
                } else {
                    MeshData::plane(*width, *depth)
                }
            }
            Self::Quad { width, height, tile, cell, surface } => {
                if *surface {
                    MeshData::quad_with(*width, *height, opts(*tile, *cell))
                } else {
                    MeshData::quad(*width, *height)
                }
            }
            Self::Sphere { radius, segments } => MeshData::sphere(*radius, *segments),
            Self::Cylinder { radius, height, segments } => MeshData::cylinder(*radius, *height, *segments),
            Self::Triangles { points } => MeshData::from_triangles(points),
            Self::RuntimeOnly | Self::Asset { .. } => return None,
        })
    }

    /// The `MeshId` an `Asset` recipe points at: loads the model if needed (cached) and finds the node's primitive.
    pub fn resolve(&self, assets: &mut Assets) -> Result<MeshId, String> {
        let Self::Asset { asset, node, primitive } = self else {
            return Err("only an asset recipe resolves to an existing mesh".into());
        };
        let model_id = assets.load_model(asset)?;
        let model = assets.model(model_id).ok_or_else(|| format!("model '{asset}' vanished"))?;
        let n =
            model.nodes.iter().find(|n| &n.name == node && n.mesh.is_some()).ok_or_else(|| format!("{asset}: no mesh node '{node}'"))?;
        let prim = n.mesh.and_then(|m| model.meshes.get(m)).and_then(|p| p.get(*primitive as usize));
        prim.map(|p| p.mesh).ok_or_else(|| format!("{asset}: node '{node}' has no primitive {primitive}"))
    }
}

impl Assets {
    /// Adds a mesh made from a recipe (for primitives and triangle lists; for an asset recipe it returns the model's
    /// own mesh id). Does not deduplicate.
    pub fn add_mesh_from(&mut self, source: &MeshSource) -> Result<MeshId, String> {
        if let MeshSource::Asset { .. } = source {
            return source.resolve(self);
        }
        let data = source.build(self)?;
        Ok(self.add_mesh_with_source(data, source.clone()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reflects_round_trip() {
        let all = [
            MeshSource::RuntimeOnly,
            MeshSource::Cube { size: 2.0 },
            MeshSource::Box { size: Vec3::new(1.0, 2.0, 3.0), tile: Some(0.5), cell: None, surface: true },
            MeshSource::Sphere { radius: 0.5, segments: 12 },
            MeshSource::Triangles { points: vec![Vec3::ZERO, Vec3::X, Vec3::Y] },
            MeshSource::Asset { asset: "assets/door.glb".into(), node: "Frame".into(), primitive: 0 },
        ];
        for s in all {
            assert_eq!(MeshSource::from_value(&s.to_value()).unwrap(), s);
        }
    }
}

//! View frustum culling.

use spark_core::{
    Aabb, Mat4, Vec3, Vec4,
};

/// Frustum planes (a, b, c, d) of a view-projection matrix (WebGPU depth 0..1, either direction), normals inward.
pub(crate) fn frustum_planes(vp: &Mat4) -> [Vec4; 6] {
    let r = [vp.row(0), vp.row(1), vp.row(2), vp.row(3)];
    [r[3] + r[0], r[3] - r[0], r[3] + r[1], r[3] - r[1], r[2], r[3] - r[2]]
}

pub(crate) fn sphere_outside(planes: &[Vec4; 6], center: Vec3, radius: f32) -> bool {
    planes.iter().any(|p| {
        let len = p.truncate().length().max(1e-6);
        (p.truncate().dot(center) + p.w) / len < -radius
    })
}

/// True if the box is completely outside one clip plane.
pub(crate) fn outside_frustum(mvp: &Mat4, bounds: &Aabb) -> bool {
    if bounds.is_empty() {
        return false;
    }
    let mut out = [true; 6];
    for corner in bounds.corners() {
        let c = *mvp * corner.extend(1.0);
        out[0] &= c.x < -c.w;
        out[1] &= c.x > c.w;
        out[2] &= c.y < -c.w;
        out[3] &= c.y > c.w;
        out[4] &= c.z < 0.0;
        out[5] &= c.z > c.w;
    }
    out.iter().any(|&o| o)
}

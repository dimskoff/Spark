//! `ReflectField` for `glam` vectors and quaternions (feature `glam`).
//!
//! `Vec2` / `Vec3` / `Vec4` behave exactly like `[f32; N]` (same `Value`, same `x`/`y`/`z`/`w` sub-paths, same
//! per-component range checks). `Quat` is reflected as **Euler angles in degrees** (`Value::Vec3([pitch, yaw, roll])`,
//! applied in Y-X-Z order, the same convention as `Object::euler`): that is the file / inspector form from
//! ADR 0003 (angles in degrees), and what a person can read and type. A quaternion decoded from Euler angles is
//! always normalized. Encoding a quaternion and decoding it again returns the same rotation up to float
//! rounding (and `pitch` is folded into -90..90, as `glam` reports it); it is not bit-exact.

use glam::{EulerRot, Quat, Vec2, Vec3, Vec4};

use crate::error::ReflectError;
use crate::field::ReflectField;
use crate::info::FieldType;
use crate::value::Value;

macro_rules! impl_glam_vec {
    ($t:ty, $n:literal) => {
        impl ReflectField for $t {
            const FIELD_TYPE: FieldType = <[f32; $n] as ReflectField>::FIELD_TYPE;
            fn to_val(&self) -> Value {
                self.to_array().to_val()
            }
            fn from_val(v: &Value) -> Result<Self, ReflectError> {
                <[f32; $n] as ReflectField>::from_val(v).map(<$t>::from_array)
            }
            fn get_path(&self, rest: &str) -> Result<Value, ReflectError> {
                self.to_array().get_path(rest)
            }
            fn set_path(&mut self, rest: &str, v: Value) -> Result<(), ReflectError> {
                let mut a = self.to_array();
                a.set_path(rest, v)?;
                *self = <$t>::from_array(a);
                Ok(())
            }
        }
    };
}
impl_glam_vec!(Vec2, 2);
impl_glam_vec!(Vec3, 3);
impl_glam_vec!(Vec4, 4);

/// `[pitch, yaw, roll]` in degrees (Y-X-Z order).
fn to_euler_degrees(q: &Quat) -> [f32; 3] {
    let (y, x, z) = q.to_euler(EulerRot::YXZ);
    [x.to_degrees(), y.to_degrees(), z.to_degrees()]
}

fn from_euler_degrees(e: [f32; 3]) -> Quat {
    Quat::from_euler(EulerRot::YXZ, e[1].to_radians(), e[0].to_radians(), e[2].to_radians())
}

impl ReflectField for Quat {
    const FIELD_TYPE: FieldType = FieldType::Vec3;
    fn to_val(&self) -> Value {
        Value::Vec3(to_euler_degrees(self))
    }
    fn from_val(v: &Value) -> Result<Self, ReflectError> {
        <[f32; 3] as ReflectField>::from_val(v).map(from_euler_degrees)
    }
    fn get_path(&self, rest: &str) -> Result<Value, ReflectError> {
        to_euler_degrees(self).get_path(rest)
    }
    fn set_path(&mut self, rest: &str, v: Value) -> Result<(), ReflectError> {
        let mut e = to_euler_degrees(self);
        e.set_path(rest, v)?;
        *self = from_euler_degrees(e);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vectors_behave_like_arrays() {
        let mut v = Vec3::new(1.0, 2.0, 3.0);
        assert_eq!(v.to_val(), Value::Vec3([1.0, 2.0, 3.0]));
        assert_eq!(Vec3::from_val(&Value::Vec3([4.0, 5.0, 6.0])), Ok(Vec3::new(4.0, 5.0, 6.0)));
        v.set_path("y", Value::Float(9.0)).unwrap();
        assert_eq!(v, Vec3::new(1.0, 9.0, 3.0));
        assert_eq!(v.get_path("z"), Ok(Value::Float(3.0)));
        assert!(Vec2::from_val(&Value::Vec3([0.0; 3])).is_err());
        assert_eq!(Vec4::from_val(&Value::Color([1.0, 0.5, 0.25, 1.0])), Ok(Vec4::new(1.0, 0.5, 0.25, 1.0)));
    }

    #[test]
    fn quat_is_euler_degrees() {
        assert_eq!(Quat::IDENTITY.to_val(), Value::Vec3([0.0; 3]));
        let q = Quat::from_euler(EulerRot::YXZ, 90f32.to_radians(), 0.0, 0.0);
        let Value::Vec3(e) = q.to_val() else { panic!("not a vec3") };
        assert!((e[1] - 90.0).abs() < 1e-3 && e[0].abs() < 1e-3 && e[2].abs() < 1e-3, "{e:?}");
        let back = Quat::from_val(&Value::Vec3([10.0, 20.0, 30.0])).unwrap();
        let e = to_euler_degrees(&back);
        assert!((e[0] - 10.0).abs() < 1e-3 && (e[1] - 20.0).abs() < 1e-3 && (e[2] - 30.0).abs() < 1e-3, "{e:?}");
        assert!((back.length() - 1.0).abs() < 1e-6);
        let mut r = Quat::IDENTITY;
        r.set_path("y", Value::Float(45.0)).unwrap();
        assert!((to_euler_degrees(&r)[1] - 45.0).abs() < 1e-3);
        assert!(r.set_path("w", Value::Float(1.0)).is_err(), "no `w`: it is three angles");
    }
}

#![allow(unused_imports)]
use spark_reflect::{Reflect, ReflectEnum};

#[derive(Reflect)]
struct A {
    #[reflect(range = 0.0..1.0)]
    x: f32,
}

fn main() {}

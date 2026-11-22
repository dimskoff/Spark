#![allow(unused_imports)]
use spark_reflect::{Reflect, ReflectEnum};

#[derive(Reflect)]
struct A {
    #[reflect(step = 0.1, step = 0.2)]
    x: f32,
}

fn main() {}

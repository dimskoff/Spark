#![allow(unused_imports)]
use spark_reflect::{Reflect, ReflectEnum};

#[derive(Reflect)]
struct A {
    #[reflect(unit = "furlongs")]
    x: f32,
}

fn main() {}

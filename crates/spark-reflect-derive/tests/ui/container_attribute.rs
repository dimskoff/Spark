#![allow(unused_imports)]
use spark_reflect::{Reflect, ReflectEnum};

#[derive(Reflect)]
#[reflect(hidden)]
struct A {
    x: f32,
}

fn main() {}

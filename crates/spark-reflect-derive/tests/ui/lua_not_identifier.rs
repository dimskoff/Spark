#![allow(unused_imports)]
use spark_reflect::{Reflect, ReflectEnum};

#[derive(Reflect)]
struct A {
    #[reflect(lua = "not valid")]
    x: f32,
}

fn main() {}

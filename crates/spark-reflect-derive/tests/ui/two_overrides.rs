#![allow(unused_imports)]
use spark_reflect::{Reflect, ReflectEnum};

#[derive(Reflect)]
struct A {
    #[reflect(color, node_ref)]
    x: [f32; 4],
}

fn main() {}

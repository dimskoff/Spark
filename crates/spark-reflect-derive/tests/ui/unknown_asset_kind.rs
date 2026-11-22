#![allow(unused_imports)]
use spark_reflect::{Reflect, ReflectEnum};

#[derive(Reflect)]
struct A {
    #[reflect(asset = "video")]
    x: String,
}

fn main() {}

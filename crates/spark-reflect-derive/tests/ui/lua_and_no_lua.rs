#![allow(unused_imports)]
use spark_reflect::{Reflect, ReflectEnum};

#[derive(Reflect)]
struct A {
    #[reflect(lua = "x", no_lua)]
    x: f32,
}

fn main() {}

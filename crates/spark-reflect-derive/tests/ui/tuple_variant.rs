#![allow(unused_imports)]
use spark_reflect::{Reflect, ReflectEnum};

#[derive(ReflectEnum)]
enum A {
    X(f32),
}

fn main() {}

#![allow(unused_imports)]
use spark_reflect::{Reflect, ReflectEnum};

#[derive(ReflectEnum)]
enum A {
    #[reflect(nme = "x")]
    X,
}

fn main() {}

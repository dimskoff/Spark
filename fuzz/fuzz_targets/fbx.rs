#![no_main]

use std::path::Path;

use libfuzzer_sys::fuzz_target;
use spark_core::assets::Assets;
use spark_core::model::ModelData;

// Any input must give Ok or Err; a panic, hang or huge allocation is a bug.
fuzz_target!(|data: &[u8]| {
    let mut assets = Assets::new();
    let _ = ModelData::load_bytes(Path::new("fuzz.fbx"), data, &mut assets);
});

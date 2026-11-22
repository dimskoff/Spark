#![no_main]

use std::path::Path;

use libfuzzer_sys::fuzz_target;
use spark_core::assets::Assets;
use spark_core::model::ModelData;

// Any input must give Ok or Err; a hang or a huge allocation is a bug. `load_bytes` turns panics inside the
// third-party `gltf` crate into errors, so those are not reported here.
fuzz_target!(|data: &[u8]| {
    let mut assets = Assets::new();
    let _ = ModelData::load_bytes(Path::new("fuzz.glb"), data, &mut assets);
});

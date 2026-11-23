# Fuzzing the model loaders

Targets (`fuzz_targets/`):

- `fbx`: binary and ASCII FBX through `ModelData::load_bytes`
- `gltf`: glTF / GLB through `ModelData::load_bytes`

The contract: any bytes give `Ok` or `Err(String)`; never a panic, hang or unbounded allocation.

```
cargo install cargo-fuzz        # once; needs a nightly toolchain
cd fuzz
cargo +nightly fuzz run fbx
cargo +nightly fuzz run gltf
```

Good seeds: put a few small `.fbx` / `.glb` files into `fuzz/corpus/fbx/` and `fuzz/corpus/gltf/`.

This directory has its own `[workspace]`, so it is not part of the engine workspace and `cargo build` / `cargo test` at the
repository root ignore it. Without cargo-fuzz, `cargo test -p spark-core --test fbx_robustness` runs deterministic
mutation loops (seeded, a few thousand mutants per format) plus targeted malformed-file cases.

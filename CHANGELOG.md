# Changelog

Spark is in 0.x: every minor version may change the Luau API. After `spark update` run `spark docs .` in your game,
read the entries below for your old version and the new one, fix the code, then raise `spark = "..."` in `game.toml`.
`spark check` reports a version mismatch under `notes`.

## Unreleased

### Behavior changes (stricter validation; check your game if it relied on the old leniency)
- **Numbers must be finite.** `NaN`, `inf` and values that overflow a float now raise an error naming the argument
  (before: `every(0/0, fn)` fired 1000 times per frame, `Mesh.cube(0/0)` made a broken mesh, `camera.fov = 0/0` blanked the screen).
  Applies to every numeric argument, vectors and colors.
- **File paths from game code stay inside the game folder.** `Texture.load`, `Model.load`, `Font.load`, `sound.play`, `Shader.load`,
  `sky { cubemap = ... }` and `require` reject absolute paths, drive letters (`C:`), UNC paths, `..` above the game folder and
  `:` (before: any file on the machine could be opened). glTF / FBX texture lookup rejects drive prefixes too.
- Unknown `--flags` when running a game are an error with a "did you mean" hint (before: ignored, a window opened).
- `obj:every(0, fn)` is an error like the global `every(0, fn)`.

### Fixes
- Materials with alpha < 0.3 and blending (alpha / additive) are no longer cut out (the 0.3 cutout applies to opaque materials only).
- `save`: a failed write no longer loses the "unsaved" mark; the file is written with fsync + atomic rename, a failure while
  playing is logged and retried every 2 s instead of stopping the game; real cycle detection (the old "cycle?" appeared on any
  table nested deeper than 32 levels); a 100k-value limit per `save.set`.
- Game packs: entry sizes are validated against the file before allocating; files over 1 GiB are refused.
- Devtools: lines over 8 MiB cut the connection; the incoming queue is bounded.
- `spark check` accepts UTF-8 files with a BOM (it reported a syntax error on every file).
- Spark Editor: untagged stderr lines (panics) are no longer shown as Info.

### Tooling
- CI runs `cargo fmt --check` and `clippy -D warnings` with no allowed lints; `rustfmt.toml` added.
### Added
- **API registry**: `docs/API_REFERENCE.md`, `docs/api.json`, `llms.txt` are generated from `docs/spark.d.luau`; tests fail when the docs and the engine disagree. `spark new` ships them plus `CLAUDE.md`.
- **Agent CLI**: `spark api search|show|list`, `spark doctor`, `spark screenshot`, `spark inspect`, `spark golden`, `spark diff`, `spark mcp` (Model Context Protocol server on stdio); `--json` on `new`, `docs`, `build`, `update`. Exit codes: 0 ok, 1 failed, 2 usage/environment.
- Range checks: `camera.fov` 1..179, `near`/`far`, mesh sizes and radii, body `mass`/`friction`.
- Devtools protocol 2: `SetObject` (edit a running object), `Hello.token`. Luau memory limit (1 GiB).
- 12 examples (`examples/01-hello` ... `showcase/`) with golden images; the old `starfall` example was removed.
- **FBX import** (`.fbx` in `Model.load` / `spawn`): binary and ASCII 7.x, hierarchy, meshes per material, skins, animation stacks,
  unit / up-axis conversion, texture lookup next to the file with automatic PBR sets (`_Normal`, `_AO`, `_Roughness`, `_Metallic`, `_Opacity`).
- `spawn(model, { node = "name" })` and `model.nodes`: use one part of a model that holds a whole kit.
- **ORM maps** (`mat:with_orm_map`, `obj.orm_map`): ambient occlusion / roughness / metallic from one texture in PBR materials.
- Opaque materials now cut out pixels with alpha < 0.3 (was 0.01): foliage no longer shows dark fringes.
### Luau API / tools
- `spark new --template 3d|2d|empty` (the flag was documented but did nothing). New games get `tests/smoke.luau`.
- `spark check` also validates every `.wgsl`, `.png` / `.jpg` and `.gltf` / `.glb` in the game, and takes `--types`
  (type-check with luau-lsp). The JSON report has a new `notes` array.
- `spark test`: `test.texts()`, `test.sees(s)`, `test.expect_text(s)`, `test.wait_text(s)` read what the game drew.
- Docs inside games and in this repo now share one name: `docs/SPARK_API.md`, `docs/SPARK_SHADERS.md`
  (this repo used `LUAU_API.md` / `SHADERS.md`).

### Fixes
- The sky gradient / cubemap was mirrored vertically (reversed-Z introduced in the renderer was not applied to the
  skybox ray). Games that worked around it by swapping `top` / `bottom` colors should undo that.

### Packaging
- Release archives contain `spark-editor` next to `spark`; every archive has a `.sha256` that the installers verify.
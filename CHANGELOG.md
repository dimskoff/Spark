# Changelog

Spark is in 0.x: every minor version may change the Luau API. After `spark update` run `spark docs .` in your game,
read the entries below for your old version and the new one, fix the code, then raise `spark = "..."` in `game.toml`.
`spark check` reports a version mismatch under `notes`.

## Unreleased

- **FBX import** (`.fbx` in `Model.load` / `spawn`): binary and ASCII 7.x, hierarchy, meshes per material, skins, animation stacks,
  unit / up-axis conversion, texture lookup next to the file with automatic PBR sets (`_Normal`, `_AO`, `_Roughness`, `_Metallic`, `_Opacity`).
- `spawn(model, { node = "name" })` and `model.nodes`: use one part of a model that holds a whole kit.
- **ORM maps** (`mat:with_orm_map`, `obj.orm_map`): ambient occlusion / roughness / metallic from one texture in PBR materials.
- Opaque materials now cut out pixels with alpha < 0.3 (was 0.01): foliage no longer shows dark fringes.
## Unreleased (0.1.x)

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
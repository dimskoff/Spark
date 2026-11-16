# Spark audit (Phase 0)

Baseline: `main` at `aa92d4b`, Windows 11, Rust 1.99.1 (pinned). Status column: **fixed** = fixed on branch
`overhaul/phase-0-1` with a regression test; **open** = found, not yet fixed; **n/a** = claim from the brief that the repository
contradicts.

How the facts were gathered: a full `cargo test --workspace` / `clippy` / `fmt --check` on a clean `main` worktree, targeted
code reading, two read-only code audits (renderer; security/editor/CLI) and a from-scratch "AI agent" trial that built three
games using only the generated project docs. Not done yet: miri, sanitizers, cargo-fuzz targets, 1k/10k/100k-object profiling,
GPU timing (listed under *Not yet measured*).

## 0. Baseline numbers

| Check | Result on `main` |
|---|---|
| `cargo test --workspace` | 183 passed, 0 failed (the brief said 175 tests; sky test **passes**) |
| `cargo clippy --workspace --all-targets` (no `-A`) | 106 warnings (73 `unit_arg` in spark-script, rest in core/render/physics) |
| `cargo fmt --all --check` | 1445 diff hunks (no `rustfmt.toml`; code uses a wide hand layout) |
| Rust code | 113 files, ~29.4k lines; largest: editor `workspace/mod.rs` 1205, `fbx.rs` 1117, `assets.rs` 909 |
| CI | 12 `-A clippy::*`, fmt disabled, no deny/audit, no MSRV job, no golden images |

## 1. Findings from the brief, re-verified

| ID | Brief claim | Verdict | Evidence | Status |
|---|---|---|---|---|
| K1 | Sky is upside down on HEAD | **n/a**: already fixed by `efbe2ee` ("fix upside-down sky under reversed-Z"); `sky_gradient_and_cubemap` passes | baseline test run | fixed upstream |
| K2 | NaN/inf accepted (`every(0/0)`, `Mesh.cube`, `camera.fov`, mass, velocity, gravity, positions) | **confirmed**: `every(0/0, fn)` fired 28,000 times in 30 frames in the agent trial; `Mesh.cube(0/0)` made a NaN mesh | `tests/scripts/validation.luau` fails on baseline with "every(NaN) was accepted" | **fixed** (`convert.rs`: `Num`/`Num64`/`ONum` argument types used by ~90 closures, `to_num`, `to_vec3`, `to_color`) |
| K3 | Asset loaders escape the game folder | **confirmed** for `Texture.load`, `Model.load`, cubemap, `Shader.load`, `sound.play`, `Font.load` (absolute, `..`, `C:`, UNC `\\host\share` = NTLM leak) | agent trial read `C:/Windows/win.ini` | **fixed**: `vfs::game_relative` / `Assets::confine` |
| K4 | `require("./C:/Windows/win")` resolves to a drive path | **confirmed** by logic (`PathBuf::join` replaces the root on Windows) | `resolve_spec` test | **fixed** |
| K5 | Editor log level bug at `process.rs:199` | **partly n/a**: the `if/else` with equal arms is not in the code; the real defect is that an untagged stderr line (panic, crash text) was classified Info | `process.rs` | **fixed** (panic -> Error, other untagged stderr -> Warn) |
| K6 | `save`: dirty flag cleared before write, no fsync | **confirmed** | `save.rs` | **fixed**: dirty cleared only after success, temp + `sync_all` + rename (+ dir fsync on Unix), non-fatal retry every 2 s |
| K7 | Update chain: installer from `main`, no signature | **confirmed**: `spark update` pipes `install.ps1`/`install.sh` from the `main` branch into a shell; the `.sha256` comes from the same release (integrity only, no authenticity); `install.sh` fails open without `sha256sum` | `update.rs:4-5,57-64`, `install.sh:78-85` | open (Phase 7) |
| K8 | Devtools unbounded line / queue; `Pack::read` allocates by index size | **confirmed** | `link.rs`, `vfs.rs` | **fixed**: 8 MiB line cap, bounded inbox (1024, back-pressure), pack entries validated against the index, 1 GiB per-file cap |
| K9a | `save.set` "cycle?" message on deep DAGs | **confirmed**: depth limit 32 reported as cycle | `save.rs` | **fixed**: real cycle detection, 64 levels, 100k-value budget |
| K9b | Recursive "in handler of event..." chains | open (low) | brief | open |
| K9c | Luau without sandbox | **confirmed**: `Lua::new()` (all stdlib). `lua.sandbox(true)` is not enabled; Luau `os`/`io` do not exist in mlua's luau build, so exposure is mainly `require`/globals mutation and memory | `lib.rs:260` | open (Phase 7, needs a mod policy) |
| K10 | Repo mess | see section 8 | | partly fixed |

## 2. New findings

Severity: **C** critical, **H** high, **M** medium, **L** low.

### Correctness and validation
| ID | Sev | Finding | Status |
|---|---|---|---|
| N1 | H | Mistyped run flag is ignored and opens a window (`spark game --headles --frames 2` hung/exit 0) | **fixed** with "did you mean" |
| N2 | H | `spark check` reports a UTF-8 BOM as a Luau syntax error on every file while `spark test` and the runtime accept it (Windows PowerShell 5.1 writes BOMs) | **fixed** (`vfs::read_string`) |
| N3 | H | Ranges are still unchecked: `camera.fov = 0`, `Mesh.cube(-1)`, negative sizes/radii, `near >= far` | open |
| N4 | M | glTF URI: `a/C:x.png` passed `safe_relative` (a later path component with a drive prefix replaces the base on Windows); FBX texture lookup had the same hole | **fixed** |
| N5 | M | `obj:every(0, fn)` was accepted (the global `every` rejected it) | **fixed** |
| N6 | H | FBX loader can panic on malformed files: unchecked `verts[cp*3]`, `influences[cp]`, overflow in `(ni+1)*3`, unbounded zlib inflate | open (fuzz target needed) |
| N7 | M | `ImageData` has public fields, so its size assert is bypassable; `width*height*4` overflows `u32` | open |
| N8 | L | Misspelled globals, API functions, fields, callbacks (`function Update`) and event names give no hint or are silent | open (Phase 2: strict globals + did-you-mean) |
| N9 | M | `spark.d.luau` misses `test.texts/sees/expect_text/wait_text`; no drift test beyond a few name checks | open (Phase 2: generate from one registry) |
| N10 | M | `test.expect_text` right after `wait_until` sees the previous frame; `restart()` is impossible in tests (no persistence test); `texts()` lists shadowed text twice | open |
| N11 | L | Headless runs are not seeded (`math.random`) | open |

### Security / robustness
| ID | Sev | Finding | Status |
|---|---|---|---|
| S1 | H | Devtools has no authentication: the editor takes the first TCP connection on its localhost port and never checks `Hello.pid` | open (token handshake, Phase 2) |
| S2 | M | `--devtools ADDR` is honoured by shipped (packed) games and connects to any host | open |
| S3 | M | `spark build` `collect()` follows symlinks without loop/out-of-root checks; `tar` is called without `--` | open |
| S4 | M | `install.ps1` renames the old exe before copying, no rollback; `install.sh` copy to `X.new` can follow a planted symlink | open |
| S5 | M | `unsafe { env::remove_var }` in `spark-render/init.rs:569` is unsound if other threads exist (parallel tests, audio) | open |
| S6 | L | Editor runs the game's Luau in-process with no `catch_unwind` and no memory limit | open |
| S7 | L | `icon.rs` uses a predictable temp path and `dangling::<u16>()` as `MAKEINTRESOURCE(2)` | open |

### Renderer (from the renderer audit; details in `docs/graphics/GAPS.md` once written)
| ID | Sev | Finding |
|---|---|---|
| R1 | H | Opaque shader discards `alpha < 0.3` in **every** blend mode: an `Alpha` material at 0.2 vanishes; the docs promise 0.01 |
| R2 | H | One sun shadow map, no cascades; 40 m default coverage |
| R3 | H | Skinning is CPU-side, the whole vertex buffer is re-uploaded per instance per frame; normals use the skin matrix instead of its inverse transpose |
| R4 | M | Normal / ORM maps are uploaded as sRGB and converted back in the shader; mips are averaged in the wrong space |
| R5 | M | 64-object uniform window per draw; every pixel loops over all (<= 64) lights; no depth pre-pass, opaque is state-sorted not front-to-back |
| R6 | M | glTF: no morph targets, metallic-roughness / occlusion / emissive textures ignored, `alphaCutoff` ignored, no vertex colors / UV1 / tangents, no KHR extensions beyond unlit; models default to non-PBR |
| R7 | M | `bloom.wgsl` jitters with time (shimmer); per-frame allocations: post-pass bind groups, light/shadow vectors, animation clones |
| R8 | L | IBL is a box-filtered LDR mip chain, no BRDF LUT; cube shadow seams |
| R9 | ok | All depth-convention sites (skybox, SSAO, motion blur, DoF, fog, culling, picking) are consistent with reverse-Z |

### Architecture
- Crate graph is clean (core has no GPU; render/window/audio/physics/script depend on core). The facade `spark` owns the
  frame loop; `ScriptGame` swaps the `World` in and out of an `Rc<RefCell>` around every Lua call.
- Hot reload drops the whole Lua state and calls `world.reset()`; there is no state carry-over (acceptable, document it).
- Monoliths: editor `workspace/mod.rs` (1205 lines: layout, dialogs, fs operations, dispatcher), `assets.rs` (three previewers),
  `viewport.rs` (runs game Luau in-process). `spark-render` was already split into modules by upstream.
- No ECS and no scene files: scene = tree of `Object` in `spark-core::scene`. Assets are addressed by path, no GUIDs, no import step.

### AI-agent experience (from the from-scratch trial; the main input to Phase 2)
All three games (3D physics cubes, 2D coin game with `save`, UI form + scenario test) were completed in 1-3 iterations. Top
frictions, ranked: unknown CLI flags ignored (N1, fixed); BOM false errors (N2, fixed); NaN accepted (K2, fixed); asset path
escape (K3, fixed); misspelled names unreported (N8); error JSON mixes message, hint and traceback with engine-internal frames;
`spark.d.luau` drift (N9); no `--help` listing subcommands; `--json` not honoured by every failure path; UI has no list or
text-input widget and tests cannot click by widget id; `docs` inconsistencies (`task.spawn` suggested in the infinite-loop
error although the API is `task(fn)`; `every(s, fn, opts)` vs `obj:every(s, fn, times)` take different third arguments;
`Texture.load` documented as "relative to the script folder"; a literal `\n` near `SPARK_API.md:743`). The trial folders were
not committed; the list above is the durable record.

## 3. Brief vs repository (discrepancies, repository wins)
- Brief: "SPARK 1.0.0 (merge #8)"; repo `Cargo.toml` says `0.1.0` and the CHANGELOG/README say 0.1. **Decision:** the version stays
  0.x; "1.0.0" in upstream's commit title is not a version. One source of truth is `[workspace.package].version`.
- Brief: README says "No editor". The README here already documents Spark Editor; the stale statement is in the brief.
- Brief: 175 tests. Actual 183 before, 190+ now.
- Brief: `_build.bat` in the root. Not present on `main`.
- Brief: 12 suppressed lints. Confirmed; all removed on this branch (4 justified `#[allow(clippy::too_many_arguments)]` and 1
  `len_without_is_empty` remain next to the code, each with a reason).
- Branding: the splash is **opt-in** (`splash = false` default, `--no-splash`), the badge only appears if the game calls
  `draw.spark_badge`. `branding.rs`'s module doc ("shown by every windowed Spark game") is stale.

## 4. Gap analysis: graphics (vs Godot 4 / Bevy / Unity URP / Unreal 5)

| Feature | Spark | Priority |
|---|---|---|
| Cascaded shadow maps, soft edges | absent (single map, 3x3 PCF) | P0 |
| GPU skinning, morph targets | absent (CPU skinning) | P0 |
| glTF PBR completeness (ORM, occlusion, emissive tex, alphaCutoff, vertex color, UV1, tangents) | partial | P0 |
| Linear data textures (normal/ORM) | wrong (sRGB upload) | P0 |
| Storage-buffer instancing, clustered lighting, depth pre-pass | absent | P1 |
| Prefiltered HDR IBL, SH irradiance, BRDF LUT, reflection probes | partial | P1 |
| Default tonemapping, auto-exposure, mip-chain bloom, TAA + motion vectors | partial / absent | P1 |
| GI (lightmaps, probes, SSGI), SSR, volumetric fog, atmosphere, clouds | absent | P2 |
| Particles / decals / terrain / water / vegetation | absent | P2 |
| Compressed textures (KTX2/BC7/ASTC), meshopt, Draco | absent | P2 |
| Render graph, GPU-driven culling / HZB, LOD beyond distance swaps | absent | P3 |
| Hardware ray tracing | not exposed by wgpu | n/a |

## 5. Gap analysis: general-purpose engine

Absent: scene/prefab files, ECS or component storage, asset GUIDs/import/cooking/streaming, navigation, animation state
machines/IK/blending beyond clips, particles, networking, localization (editor only), retained-mode UI/layout, input action
maps, audio effects/occlusion, job system (Luau is single-threaded by design), Web/mobile targets. Present and good:
timers/tasks/events, tweens, rapier physics with character controller and picking, kira audio with buses, save, packed
single-exe builds, hot reload, headless screenshot/scenario tests.

## 6. Editor (current vs target)

Today: project manager, explorer, code editor with Luau highlighting, console, problems, debugger/stats, scene tree and a
viewport that runs the game on the editor GPU, asset previews, live link to a running game. Not a scene editor: no inspector
write-back, gizmos, undo/redo, prefabs, material graph, animation timeline, profiler. The target in the brief (Godot-class) is a
multi-month track that depends on scene serialization and a reflection layer (Phase 3) first.

## 7. Performance

Not yet measured (needs the benchmark scenes from Phase 4): frame time at 1k/10k/100k objects, draw calls, allocations/frame,
cold start, exe size. Known structural limits from reading: 64-object uniform window, per-pixel loop over <= 64 lights, per-frame
Vec/HashMap allocations in the lighting, post and animation paths (see R7), CPU skinning.

## 8. Repository and documentation

- Duplicated and stale docs: `docs/SPARK_API.md` is copied verbatim into every game (`include_str!`), so there is no drift between
  repo and generated docs, but there is no test tying `SPARK_API.md` / `spark.d.luau` to the Rust API (N9).
- Root: `install.ps1`/`install.sh` (comments had literal `\n`, fixed), `README.md`/`README.ru.md` (must stay in sync),
  `CHANGELOG.md` (two "Unreleased" headings, merged on this branch).
- Examples `demo`, `hello` (Rust), `starfall`: working, not minimal, no per-feature examples, no screenshot checks. Replacement plan in ROADMAP Phase 1b.
- Remote branches on `origin` (`agent-b/...`, `pr/*`, `spark/*`, `wip/review-scripts`, `fix/core-improvements`) are historical;
  they are not touched here and should be reviewed and pruned by the owner.

## Not yet measured / not yet done
miri, sanitizers, cargo-fuzz (glTF/FBX/pack/devtools/JSON/images/audio), `cargo deny`/`audit` (tools not installed in this
environment), profiling, Linux/macOS runs (CI only), GPU timing.

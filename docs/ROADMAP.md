# Spark roadmap

Order follows risk and dependency: trust and tooling first (every later phase is tested through them), then the agent
experience (the product's reason to exist), then engine capability, then graphics and editor which both need scene
serialization and a reflection layer. Findings are in [audit/AUDIT.md](audit/AUDIT.md). "Done" always means: a test or measurement
exists and was run.

| Phase | Goal | Depends on | Done when (measurable) |
|---|---|---|---|
| 0 Audit | Facts, not assumptions | - | AUDIT.md written; baseline numbers recorded |
| 1a Hygiene | Strict CI, known bugs closed | 0 | fmt + `clippy -D warnings` (no global `-A`) green; every fixed finding has a test that failed before |
| 1b Repo and examples | Small, true repository | 1a | 8-12 one-feature examples + 3-4 showcases, each run by `spark check` in CI; every deletion in `audit/CLEANUP.md` |
| 2 Agent experience | An agent verifies instead of guessing | 1a | API registry generates `SPARK_API.md`, `spark.d.luau`, `llms.txt` and a drift test fails CI; `--json` on every command with stable exit codes; `SPK-xxxx` error codes; `spark mcp`; benchmark of 15-20 tasks, >= 80% pass within 3 iterations |
| 3 Engine core | Scenes, assets, animation, UI | 2 | scene/prefab file format with migrations; asset GUIDs + import cache; async loading; animation state machine; retained UI |
| 4 Renderer 2.0 | Modern PBR image, stable on llvmpipe | 1a, 3 (assets) | CSM, GPU skinning, linear data textures, alpha fix, IBL, tonemapping default, TAA; golden screenshots with tolerance; GPU pass timings; perf table per quality profile |
| 5 Editor | Human and agent driven | 3 | create scene, assign materials, animate, run, debug, export without editing files; undo/redo for every action |
| 6 Docs | Generated, executable, checked | 2 | docs site builds in CI; all doc examples executed; link check green |
| 7 Security and release | Verifiable supply chain | 1a | signed releases + `SHA256SUMS`; installers verify signature; fuzz targets (glTF, FBX, pack, devtools, JSON, images, audio) run on a schedule; Luau sandbox + mod policy; SBOM; performance budgets in CI |

## Risks
- **Scope.** Phases 3-5 are each months of work for a team; this plan is ordered so that stopping after any phase leaves a coherent
  product. Phase 4 and 5 items are individually shippable.
- **Compatibility.** Public Luau API and `game.toml` only change through deprecation + CHANGELOG + `docs/MIGRATION.md`.
  Validation tightening (NaN, paths, unknown flags) is a deliberate behavior change recorded in the CHANGELOG.
- **Renderer without a GPU in CI.** Golden images run on llvmpipe (Linux) and are compared with a tolerance; real-GPU
  differences are tracked, not blocked.
- **Reflection.** An inspector, serialization, docs generation and MCP all need one registry of the Luau API / components.
  Phase 2 builds the API registry first; scene reflection follows in Phase 3.
- **Tooling gaps in this environment.** `cargo-deny`, `cargo-audit`, `cargo-fuzz`, `luau-lsp` are not installed here;
  they are wired into CI but could not be run locally.

## Phase 1a checklist (status on branch `overhaul/phase-0-1`)
- [x] NaN / infinity / overflow rejected at every numeric API argument; vectors and colors checked
- [x] Asset path jail (textures, models, cubemaps, shaders, sounds, fonts), `require` drive prefixes, glTF/FBX drive prefixes
- [x] `save`: durable atomic write, retry, cycle detection, size budget
- [x] Pack index validation, per-file size cap; devtools line cap and bounded inbox
- [x] Unknown run flags are errors with a suggestion; `spark check` accepts BOM files; editor log level for panics
- [x] `cargo fmt` applied (`rustfmt.toml`, width 140); `clippy -D warnings` clean without `-A`
- [x] CI: fmt + clippy strict
- [ ] CI: `cargo deny` / `audit`, MSRV job, golden screenshots, fuzz smoke, API drift test
- [ ] Range validation (fov, sizes, near/far), FBX hardening, `ImageData` invariants
- [ ] Crash handler report file, `forbid(unsafe_code)` where possible, remove `env::remove_var` unsafe
- [ ] Examples replacement (1b)
- [ ] `SECURITY.md`, `CONTRIBUTING.md`, `CODE_OF_CONDUCT.md`, `ARCHITECTURE.md`, version policy

## Next steps in order
1. Range validation + FBX/ImageData hardening with fuzz-style tests.
2. API registry (`spark-api` data in Rust) -> generated docs, `.d.luau`, `llms.txt`, drift test.
3. Strict globals / did-you-mean, `SPK-` error codes, `--json` everywhere, `spark doctor`, `spark inspect`.
4. Replace examples; golden screenshots in CI.
5. `spark mcp` over the existing devtools protocol (versioned, token-authenticated).

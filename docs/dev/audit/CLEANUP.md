# Cleanup log

Every removal or move, with the reason.

| Change | Reason |
|---|---|
| 12 global `-A clippy::*` flags removed from `.github/workflows/ci.yml` | Lints were allowed globally "until the crates are clean". All crates are clean now (`clippy -D warnings`). Five justified local `#[allow]` remain, each with a comment. |
| `cargo fmt` applied to the whole workspace, `rustfmt.toml` added (`max_width = 140`, `use_small_heuristics = "Max"`) | CI had formatting disabled because no config matched the hand-kept layout; 81 files changed mechanically in one commit so it can be skipped in blame. |
| `fmt` CI step enabled | See above. |
| Literal `\n` text in comments of `install.ps1` and `.github/workflows/release.yml` replaced by real line breaks | Broken line wraps from an earlier edit; comments ran together. |
| `unit_arg` rewrites (`Ok(w.scene.ambient = c)` -> statement + `Ok(())`) done by `cargo clippy --fix` | 73 mechanical warnings. |
| `docs/dev/audit/` and `docs/dev/ROADMAP.md` added | Audit record and plan requested for the overhaul. |

Not removed (decision recorded): `examples/demo`, `examples/hello`, `examples/starfall` stay until their replacements (Phase 1b)
exist; deleting them first would leave the repository without any runnable sample and CI without `spark check` coverage of
shaders and sounds. `_build.bat` mentioned in the brief is not in the repository.

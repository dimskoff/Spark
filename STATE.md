# STATE (handoff)

Branch: `overhaul/phase-0-1` (from `main` @ aa92d4b). Nothing is pushed to `main`.

## Done
- Phase 0: `docs/audit/AUDIT.md`, `docs/ROADMAP.md`, `docs/audit/CLEANUP.md`.
- Phase 1a (see checklist in ROADMAP): finite numbers, path jail, save durability, pack/devtools limits, unknown flags,
  BOM in check, editor log level, fmt + clippy -D warnings, strict CI, SECURITY/CONTRIBUTING.

## Commands
```
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo test -p spark-script --test flow validation     # NaN + path regression script
```

## In progress / next (ROADMAP "Next steps")
1. Range validation (fov, sizes, near/far); FBX panics (`fbx.rs` control-point indexing, inflate limit); `ImageData` invariants.
2. API registry -> generated SPARK_API.md / spark.d.luau / llms.txt + drift test.
3. Strict globals + did-you-mean, `SPK-` codes, `--json` on all commands, `spark doctor/inspect`.
4. Replace examples (see CLEANUP.md for why they are still there); golden screenshots.
5. `spark mcp`, devtools token auth.

## Notes
- Scratch worktree `../Spark-baseline` (clean `main`) was used for baseline numbers; remove with `git worktree remove ../Spark-baseline`.
- Tools not installed locally: cargo-deny, cargo-audit, cargo-fuzz, luau-lsp.

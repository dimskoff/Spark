# Contributing

```sh
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

All three must pass; CI runs them on Windows, Linux and macOS. The toolchain is pinned in `rust-toolchain.toml`.

- **Fix a bug test-first.** Add the failing test (a Luau contract script in `crates/spark-script/tests/scripts/` or a unit test),
  see it fail, then fix.
- **No lint is silenced globally.** A justified `#[allow(clippy::...)] // reason` next to the code is acceptable.
- **No `unwrap` / `expect` on user data** (files, scripts, network). Return an error with the file, the cause and a hint.
- **Files from game code go through `spark_core::vfs`** (`game_relative` / `jail` / `read`), never `std::fs` directly.
- **Numbers from Luau go through `convert::Num` / `to_num`** so NaN and infinity are rejected.
- **Public Luau API and `game.toml` changes** need a CHANGELOG entry and, if they break, a deprecation first.
- Code, comments and docs are in English; keep `README.ru.md` in step with `README.md`.
- Architecture decisions go in `docs/dev/adr/NNNN-title.md`.

# ADR 0002: Luau hardening without `lua.sandbox(true)`

**Context.** Mods and agent-written code are semi-trusted. Luau (mlua) has no `io` / `os.execute`; the risks are memory exhaustion, endless loops, and file access through Spark's own API.
**Tried.** `lua.sandbox(true)` after installing the API. 9 of 15 script contract tests failed: globals assigned from required modules hit a read-only table, and writes to userdata properties through globals broke. It needs a redesign of how modules and globals are installed.
**Decision for now.** (1) memory limit 1 GiB per game (`set_memory_limit`, test `memory_bomb.luau`), (2) existing 5 s callback watchdog, (3) path jail for every file API, (4) finite-number validation. `sandbox(true)` stays on the roadmap (Phase 7) together with a mod policy.

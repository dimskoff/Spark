# Security policy

## Reporting a vulnerability
Open a private security advisory on GitHub (Security > Report a vulnerability) for this repository. Please do not file a public
issue for something exploitable. Include the Spark version (`spark --version`), OS, and a minimal game folder or file that
reproduces it. You can expect an acknowledgement within a week.

## What counts
- A game script reading or writing files outside its game folder (and the `save` folder)
- Crashes, hangs or unbounded memory from untrusted files: glTF/FBX models, images, audio, game packs, `save.json`, devtools traffic
- Tampering risks in `spark update`, the installers and release archives

## Trust model today
- Game scripts are trusted code (they run Luau with the full Spark API). Asset and `require` paths are confined to the game
  folder; the Luau VM is not yet sandboxed against memory exhaustion (planned: Phase 7 in `docs/dev/ROADMAP.md`).
- Devtools listens on `127.0.0.1` only and has no authentication yet (planned: token handshake).
- Release archives carry a `.sha256` published next to them; signatures are not available yet. `spark update` runs the installer
  from the `main` branch (planned: pinned to the release tag and signature-verified).

Known open issues are listed in `docs/dev/audit/AUDIT.md`.

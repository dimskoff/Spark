# Releasing Spark

1. Update `version` in `Cargo.toml` (`[workspace.package]`) and the `CHANGELOG.md` section; commit; tag `vX.Y.Z`; push the tag.
2. On GitHub: Releases > Draft a new release for that tag > Publish. `.github/workflows/release.yml` builds the archives
   (Windows x64 zip, Linux x64 tar.gz, macOS arm64 tar.gz, each with `spark` and `spark-editor`), a `.sha256` per archive, one
   `SHA256SUMS`, the installers (`install.sh`, `install.ps1`) and, when a signing key is configured, `SHA256SUMS.minisig`.
3. `spark update` and the one-line installers download the installer **of that release** (not from `main`), then the archive,
   and refuse to install when the SHA-256 does not match (or no SHA-256 tool exists).

## One-time setup: signing key (needs the repository owner)
```sh
minisign -G -p spark.pub -s spark.key        # choose an empty password (CI cannot type one)
```
- Add the content of `spark.key` as the repository secret `MINISIGN_KEY`. Keep a backup offline; never commit it.
- Publish `spark.pub` (README and the website). Users verify with `minisign -Vm SHA256SUMS -p spark.pub`.
- Until the secret exists, releases are unsigned and the checksum only protects against corruption, not against a
  compromised release page. The roadmap item "built-in signature verification in `spark update`" needs the public key
  embedded in the binary and is blocked on this step.

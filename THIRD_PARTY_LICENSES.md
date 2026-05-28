# Third-Party Dependency Licensing

`moq-cpp` is consumed as a compiled library that **statically links its entire
Rust dependency tree** into the shipped artifact. The repository `LICENSE` (MIT)
covers only the moq-cpp glue code, so complete attribution for the bundled
crates is generated separately.

## Authoritative attribution: `THIRD-PARTY-NOTICES.txt`

`THIRD-PARTY-NOTICES.txt` reproduces the full license text of every crate
linked into the library. It is generated from `Cargo.lock` by
[`cargo-about`](https://github.com/EmbarkStudios/cargo-about) using `about.toml`
(config) and `about.hbs` (template), and is the authoritative attribution file.

The build emits it automatically:

- CMake regenerates it during the build when `cargo-about` is installed, and
  installs it to `share/moq-cpp/THIRD-PARTY-NOTICES.txt` (plus a copy next to the
  built library). Packagers should ship this file alongside the binary.
- A copy is committed at the repository root, both as a reviewable artifact and
  as a fallback when `cargo-about` is not available. CI regenerates it on every
  run and fails if the committed copy is stale or if a crate has a missing or
  unaccepted license.

### Regenerating

```bash
cargo install cargo-about --locked --features cli
cargo about generate about.hbs -o THIRD-PARTY-NOTICES.txt
```

Commit the regenerated `THIRD-PARTY-NOTICES.txt` whenever the dependency tree
changes (e.g. after a `Cargo.lock` update or a dependency bump).

## What is covered

`about.toml` resolves the dependency graph for the platforms we ship
(Windows / Linux / macOS, x86_64 and aarch64) and lists only crates actually
linked for those targets. Dev-dependencies are excluded. The bundled crates
currently resolve to these licenses:

| License | Crates |
| --- | --- |
| MIT | 199 |
| Unicode-3.0 | 19 |
| ISC | 4 |
| Apache-2.0 | 3 |
| BSD-3-Clause | 1 |
| OpenSSL | 1 |

Notable crates with non-trivial licensing:

- **`aws-lc-sys`** embeds AWS-LC / BoringSSL and is dual-attributed under the
  **ISC** and **OpenSSL** licenses (its `aws-lc-rs` crate is the default rustls
  crypto provider on every platform we ship).

### Crates that are *not* linked on shipped platforms

`Cargo.lock` lists ~301 crates, but several are only pulled in for targets we do
not ship (e.g. wasm), so they are intentionally absent from the notice file:

- **`ring`** — on all shipped targets the crypto backend resolves to
  `aws-lc-rs`, not `ring`.
- **`webpki-root-certs`** (Mozilla CA data, **MPL-2.0**) — `rustls-platform-verifier`
  uses the native OS certificate verifier on Windows / macOS / Linux, so the
  bundled MPL-2.0 root store is not compiled in.

If a downstream consumer ships `moq-cpp` for a platform that uses these crates,
add that target to `about.toml` and regenerate so they are attributed.

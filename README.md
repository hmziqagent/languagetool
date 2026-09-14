# OpenGrammar-RS

A Rust-native reimplementation of LanguageTool's API-facing compute engine,
aiming for LanguageTool XML/resource/test compatibility and equivalent
`/v2/check` output — with **no Java dependency at runtime**.

Out of scope by design: AI/LLM correction, desktop apps, LibreOffice
integration, and any JVM embedding or Java subprocess. The upstream Java
codebase is used as a **read-only reference** for rules, resources, and tests.

## Status

Work in progress. The live dashboard is [docs/PLAN.md](docs/PLAN.md); each
phase has a detailed plan under [docs/plans/](docs/plans/), and
[docs/plans/compatibility-matrix.md](docs/plans/compatibility-matrix.md)
tracks parity with upstream rule by rule.

Summary: the API shell, core text model, XML rule engine, and 13 English
native rule structs are functional; only English is implemented so far
(migration order: en → de → fr → es → pt → nl → pl → uk → ar). Parity is
asserted by the workspace test suite, not yet measured against the Java
server.

## Workspace layout

11 crates, layered bottom-up (dependency direction: `↑` depends on):

```text
og-core (leaf, no internal deps)
  ↑
og-xml, og-tokenizer, og-tagger, og-spell, og-languagemodel (mid-level)
  ↑
og-rules, og-langs (composition)
  ↑
og-api, og-test-runner, og-compat (top-level integration)
```

## Build, test, run

Requires a stable Rust toolchain (MSRV 1.95, edition 2024).

```sh
cargo build --workspace
cargo test --workspace
cargo fmt --check
cargo clippy --workspace
```

Run the HTTP server (binary `og-server` from the `og-api` crate, listens on
port 8081):

```sh
cargo run -p og-api
curl -s "http://localhost:8081/v2/check?text=teh+house&language=en-US"
```

## LanguageTool resources

Rules and dictionaries are loaded at runtime from the upstream LanguageTool
Java checkout — the read-only `languagetool/` directory at the repo root.
The resource root is the `languagetool-language-modules` directory inside
it, resolved by `og_spell::resource::lt_resource_root()` in this order:

1. The `OG_LT_RESOURCE_ROOT` environment variable (absolute path to the
   `languagetool-language-modules` directory).
2. Ancestor walk from the current working directory, looking for
   `languagetool/languagetool-language-modules`.
3. Ancestor walk from the compile-time manifest directory (keeps tests
   working from any cwd).

When no root is found, data-backed rules degrade gracefully to empty data
and tests that need real resources skip themselves; nothing panics. The
upstream commit the port is checked against is frozen in
[docs/plans/compatibility-matrix.md](docs/plans/compatibility-matrix.md).

## CI

`.github/workflows/ci.yml` runs on every push and pull request:

- `cargo fmt --check`
- `cargo clippy --workspace`
- `cargo test --workspace`

## License

LGPL-2.1 — see [LICENSE](LICENSE). This project reimplements and reads
resources from [LanguageTool](https://github.com/languagetool-org/languagetool),
which is also licensed LGPL-2.1+.

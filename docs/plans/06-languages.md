# Language Modules

| | |
|---|---|
| Phases covered | 10 (language modules) |
| Status | 5% |
| Last verified | 2026-09-14 |

## Requirements

### Phase 10 — Language modules

Repeat per language:

```txt
load resources
run XML validation
run pattern tests
run spelling tests
run full check golden tests
port missing native rules
```

Deliverable: language marked parity_pass in the compatibility matrix.

Each language module (og-langs/<lang>/) contains:

```txt
grammar.xml
style.xml
disambiguation.xml
spell dictionaries
tagger dictionaries
abbreviations
native Rust rule ports
tests
```

### Language migration order (detail)

Ordered list lives in [PLAN.md](../PLAN.md). Rationale: English/German/French/Spanish will expose most engine features early — tokenization, XML rules, morphology, compound words, spelling, style rules, and complex rule classes. German in particular forces compound-word handling.

## Current state

- Only `crates/og-langs/src/en/` exists. 1 of ~37 LT language modules; 0 code for de/fr/es/pt/pl/nl and all others.
- Language inventory: og-core's `Language` enum has 50 variants (incl. `Other(String)`); `Language::all_languages()` returns 49 languages. `LanguageRegistry` (registry.rs) is data-only — it stores Language values and cannot create engines.
- No generic engine factory — only `LanguageEngine::english()`.
- English engine wiring (engine.rs:88-263) loads: grammar.xml, disambiguation.xml, dict_decoded.txt, added.txt, uncountable.txt, partlycountable.txt, spelling dictionaries. The API layer maps "en", "en-US", "en-GB" to three separate `LanguageEngine::english()` instances; all other languages → 400 "Unsupported language".

**Runtime path problems (top blocker #1):**
- `find_lt_resource_dir()` (engine.rs:370-382) probes `../languagetool-language-modules`, `../../languagetool-language-modules`, `/home/agent/languagetool/languagetool-language-modules` — none exist on this machine, so at runtime the engine loads NO grammar.xml/disambiguation.xml/tagger or spelling dictionaries.
- 14 `include_str!` sites in `crates/og-langs/src/en/` (wrong_word_context, compound, contraction, dash, diacritics, plain_english, specific_case, word_coherency, simple_replace ×6) resolve to `/Users/hmziq/os/rustslop/languagetool-language-modules/...` — one level ABOVE the repo — while the Java checkout lives at `<repo>/languagetool/languagetool-language-modules/...`. `cargo test --workspace` exits 101 on these.
- Resources are loaded from an external LT directory (not vendored) — the project only builds/runs on the filesystem layout it was developed in.

## Gaps

- 36 languages with zero implementation.
- No single configurable LT resource root (fix for all path problems above).
- style.xml never loaded; no abbreviation loading; no grammar_custom.xml support.
- Registry disconnected from engine creation; no `create_engine(language_code)` factory.

## Next steps

- Implement a single configurable LT resource root (env var / config) and point the 14 `include_str!` sites and `find_lt_resource_dir()` at it — unblocks the build, all tests, and runtime loading.
- Vendor or firmly locate the English resources; add a resource-presence smoke test.
- Add a generic engine factory and connect the registry.
- Start German per the migration order (compound words will stress og-spell).

# PLAN.md — OpenGrammar-RS Master Index

## Project definition

Pure plan goal:

```txt
Goal:
Rewrite LanguageTool compute/API engine in Rust.

No AI.
No product extension plan.
No desktop.
No LibreOffice.
No Java runtime.
No Java wrapper.
No custom simplified clone as the final goal.

Success = Rust engine passes LanguageTool-compatible tests and produces equivalent /v2/check output.
```

Final project definition:

```txt
A Rust-native reimplementation of LanguageTool's API-facing compute engine,
with LanguageTool XML/resource/test compatibility,
excluding desktop, LibreOffice, Java runtime, and non-compute integrations.
```

Success criteria:

```txt
1. Same /v2/check API shape.
2. Same rule IDs where resources are ported.
3. Same offsets.
4. Same suggestions where deterministic.
5. Same test suite behavior.
6. Same multilingual resource model.
7. No Java dependency at runtime.
```

Permanently ignored (never in scope for this rewrite):

```txt
No AI fallback
No LLM correction
No extension plan
No desktop
No LibreOffice
No Java server
No Java subprocess
No JVM embedding
No TOML/YAML grammar format
No "simplified product MVP" as the final architecture
```

## How to use these plans

This file is the single entry point: it holds the project definition, the live status dashboard, and the top blockers. Each sub plan under `plans/` owns one slice of the 13-phase execution plan (phases 0-12) and is written as Requirements / Current state / Gaps / Next steps. `plans/compatibility-matrix.md` is the living Phase 0 deliverable (language modules, English Java rule port matrix, pipeline and API parity tables) and is updated in place — never duplicate it elsewhere. When status changes, update the owning sub plan and the dashboard table here together.

| Sub plan | Phases / scope covered |
|----------|------------------------|
| [plans/01-architecture.md](plans/01-architecture.md) | Cross-cutting: rewrite scope, workspace layout + per-crate roles, migration strategy, dependency architecture, hardest parts, source citations |
| [plans/02-engine-foundation.md](plans/02-engine-foundation.md) | Phase 2 (core text model), Phase 3 (tokenizer/sentence splitter) |
| [plans/03-xml-rule-engine.md](plans/03-xml-rule-engine.md) | Phase 4 (XML parser), Phase 5 (pattern rule engine), Phase 8 (disambiguation) |
| [plans/04-spelling-morphology.md](plans/04-spelling-morphology.md) | Phase 6 (spellchecker), Phase 7 (POS/morphology/tagger), og-languagemodel |
| [plans/05-native-rules.md](plans/05-native-rules.md) | Phase 9 (native rule ports) |
| [plans/06-languages.md](plans/06-languages.md) | Phase 10 (language modules) |
| [plans/07-testing-compatibility.md](plans/07-testing-compatibility.md) | Phase 0 (source audit), Phase 11 (test suite parity), compat harness |
| [plans/08-performance.md](plans/08-performance.md) | Phase 12 (performance) |
| [plans/09-api-parity.md](plans/09-api-parity.md) | Phase 1 (API shell) |
| [plans/compatibility-matrix.md](plans/compatibility-matrix.md) | Living Phase 0 deliverable: language modules table, English Java rules port matrix, core pipeline parity, API endpoint parity |

## Current status dashboard

| Phase | Status | % | Notes | Sub plan |
|-------|--------|---|-------|----------|
| 0 — Source audit | STARTED | 40% | Matrix now lives at plans/compatibility-matrix.md | [07](plans/07-testing-compatibility.md) |
| 1 — Rust API shell | MOSTLY DONE | 80% | 5 endpoints live, but param wiring regressed — parsed only | [09](plans/09-api-parity.md) |
| 2 — Core text model | MOSTLY DONE | 80% | All 17 types exist; metadata/lifecycle gaps remain | [02](plans/02-engine-foundation.md) |
| 3 — Tokenizer | PARTIAL | 75% | URL/email detection done; no SRX, no hyphenation | [02](plans/02-engine-foundation.md) |
| 4 — XML parser | MOSTLY DONE | 85% | No unify/phrases/while; style.xml never loaded | [03](plans/03-xml-rule-engine.md) |
| 5 — Pattern rule engine | FUNCTIONAL | 80% | Regex cache added; many filters still stubs | [03](plans/03-xml-rule-engine.md) |
| 6 — Spellchecker | PARTIAL | 50% | Word-list dict + length-filtered suggestions; no Hunspell .aff | [04](plans/04-spelling-morphology.md) |
| 7 — POS/morphology | PARTIAL | 65% | Dictionary + heuristics only; no statistical tagger | [04](plans/04-spelling-morphology.md) |
| 8 — Disambiguation | PARTIAL | 65% | English XML disambig works; unify parsed but no-op | [03](plans/03-xml-rule-engine.md) |
| 9 — Native rule ports | ACTIVE | 65% | 13 English rule structs all wired; 18 of 37 Java rule classes unported (13 'Not started' matrix rows) | [05](plans/05-native-rules.md) |
| 10 — Language modules | ENGLISH ONLY | 5% | English only, 1 of ~37 | [06](plans/06-languages.md) |
| 11 — Test suite parity | BLOCKED | 15% | 477 tests exist, 0 runnable in this checkout (build broken); no fixtures/CI | [07](plans/07-testing-compatibility.md) |
| 12 — Performance | PARTIAL | 35% | Rayon + first-token index + regex cache + length-filtered suggestions | [08](plans/08-performance.md) |

**Workspace:** 11 crates, 23,641 lines of Rust.

**Tests:** 477 test functions (15 `#[ignore]`) — 0 can run in this checkout because the build is broken (blocker #1).

Last verified: 2026-09-14

## Top blockers

1. **Workspace does not compile on this checkout.** `cargo test --workspace` exits 101 — og-langs fails with 14 `include_str!` errors (all in `crates/og-langs/src/en/`: wrong_word_context, compound, contraction, dash, diacritics, plain_english, specific_case, word_coherency, simple_replace ×6). The paths resolve to `/Users/hmziq/os/rustslop/languagetool-language-modules/...` (one level ABOVE the repo) but the Java checkout actually lives at `<repo>/languagetool/languagetool-language-modules/...`. Additionally `find_lt_resource_dir()` (og-langs engine.rs:370-382) probes `../languagetool-language-modules`, `../../languagetool-language-modules`, `/home/agent/languagetool/languagetool-language-modules` — none exist on this machine, so at runtime the engine would load NO grammar.xml/disambiguation.xml/tagger or spelling dictionaries. og-spell test paths also hardcode `/home/agent/...`. Net: 477 test functions exist but 0 can run in this checkout. The project only builds on the filesystem layout it was developed in. Fix: single configurable LT resource root. Details: [06](plans/06-languages.md), [07](plans/07-testing-compatibility.md).

2. **API parameter regression.** og-api's `handle_check` (handlers.rs:10-64) now acts on ONLY `text`/`data` and `language`. `motherTongue`, `enabledRules`, `disabledRules`, `enabledCategories`, `disabledCategories`, `level`, `picky` are parsed but ignored — the per-language engine refactor (commit c644300) dropped the wiring that commit 1c4dc4c had added. Details: [09](plans/09-api-parity.md).

3. **No compatibility harness.** og-compat has comparison types only — it cannot run Java LT or the Rust server (no subprocess, no HTTP client). Parity is asserted, never measured. Details: [07](plans/07-testing-compatibility.md).

4. **36 languages with zero implementation.** Only `crates/og-langs/src/en/` exists; no generic engine factory; the registry is data-only. Details: [06](plans/06-languages.md).

5. **No SRX sentence splitting; no statistical POS tagger.** LT's splitter is driven by ~50 SRX regex rules; LT's tagger uses a trigram HMM. Details: [02](plans/02-engine-foundation.md), [04](plans/04-spelling-morphology.md).

6. **No golden fixtures, no CI, no LICENSE file, no root README.** No `.github/` CI, no clippy/rustfmt config; LGPL-2.1 is declared only in Cargo.toml. Details: [07](plans/07-testing-compatibility.md), [01](plans/01-architecture.md).

## Language migration order

Do not port all languages at once. Use this order:

```txt
1. English
2. German
3. French
4. Spanish
5. Portuguese
6. Dutch
7. Polish
8. Ukrainian
9. Arabic/Urdu/etc.
```

Reason: English/German/French/Spanish will expose most engine features early — tokenization, XML rules, morphology, compound words, spelling, style rules, and complex rule classes. Detail: [06](plans/06-languages.md).

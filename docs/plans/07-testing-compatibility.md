# Testing & Compatibility

| | |
|---|---|
| Phases covered | 0 (source audit), 11 (test suite parity), compat harness |
| Status | Phase 0: 40% · Phase 11: 15% |
| Last verified | 2026-09-14 |

## Requirements

### Phase 0 — Source audit

```txt
Freeze upstream LanguageTool commit.
List all Maven modules.
List compute-related modules.
List non-compute modules to ignore.
List all languages.
List all XML files.
List all Java Rule classes.
List all tests.
```

Deliverables:

```txt
COMPATIBILITY_MATRIX.md
LANGUAGE_MODULES.md
JAVA_RULE_PORT_LIST.md
TEST_PORT_LIST.md
```

### Phase 11 — Full test suite parity

```txt
Port JUnit tests to Rust integration tests.
Run Java and Rust golden comparison.
CI enforces parity.
```

Deliverable:

```txt
cargo test passes.
og-compat reports acceptable parity.
```

### Test strategy

The real metric is not "does it compile?". It is:

```txt
For the same input,
same language,
same enabled/disabled rules,
same mother tongue,
same picky mode/settings,

Rust output == Java output
```

Test categories (12):

```txt
1. XML validation tests
2. XML rule example tests
3. Pattern rule tests
4. Sentence tokenizer tests
5. Word tokenizer tests
6. POS tagger tests
7. Disambiguator tests
8. Spellchecker tests
9. Java-rule-port tests
10. Full /v2/check golden tests
11. Cross-language regression tests
12. Performance tests
```

Context from LT's build/test logs: language rule testing includes XML validation for grammar.xml/style.xml, remote rule filters, pattern rule tests, rule ID uniqueness, and loading thousands of rules per language (one reported English run loaded 6117 rules).

Golden output test design — generate expected output by running official Java LanguageTool at the frozen commit, then Rust must match:

```txt
fixtures/
  en/
    input_001.txt
    expected_java.json
    expected_rust.json
  de/
  fr/
  es/
```

Compare:

```txt
rule.id
message
shortMessage
offset
length
replacements
category
issueType
context
```

Offsets must be exact.

### Compatibility harness CLI design

```bash
og-compat compare \
  --java-lt ./languagetool.jar \
  --rust-og ./target/release/og-server \
  --language en-US \
  --input fixtures/en/common_errors.txt
```

Output:

```txt
PASS: same matches
FAIL: offset mismatch in RULE_ID
FAIL: replacement mismatch in SPELLING_RULE
FAIL: Java found rule X, Rust missed it
FAIL: Rust found extra rule Y
```

This harness is your project manager.

## Current state

**Test counts (2026-09-14), `#[test]` per crate / ignored:**

| Crate | Tests / ignored | Coverage quality |
|-------|-----------------|------------------|
| og-core | 0 / 0 | No unit tests |
| og-api | 6 / 0 | tokio::test integration tests in tests/ — the workspace's only tests/ dir integration tests; basic happy path |
| og-compat | 9 / 0 | Comparison logic |
| og-langs | 58 / 0 | Engine, native rules |
| og-languagemodel | 0 / 0 | No tests |
| og-rules | 60 / 2 | Pattern matching, native rules, text-level rules |
| og-spell | 6 / 0 | Basic detection, suggestions |
| og-tagger | 40 / 0 | English tagger, disambiguator, chunker |
| og-test-runner | 23 / 13 | XML examples, real grammar.xml regression |
| og-tokenizer | 185 / 0 | Very thorough sentence + word tokenizer tests |
| og-xml | 90 / 0 | Parser, compiler, disambig parser |
| **Total** | **477 / 15** | **0 runnable in this checkout** (build broken — PLAN.md blocker #1) |

**Phase 0 status:** the compatibility matrix now exists as the living deliverable at [compatibility-matrix.md](compatibility-matrix.md) (covers COMPATIBILITY_MATRIX.md + the English Java rule port list). `LANGUAGE_MODULES.md` and `TEST_PORT_LIST.md` do not exist as documents. The upstream LT commit has not been formally frozen.

**og-test-runner (working):**
- XML example test runner: parses grammar.xml, runs correct/incorrect/triggers_error examples.
- Marker extraction and offset verification.
- Full tokenization pipeline with 28 hand-coded disambiguation rules.
- Can load and run real English grammar.xml (verified 2026-09-14: 2,468 category-level entries = 1,502 standalone rules + 966 rulegroups; 5,545 total `<rule>` elements).

**og-compat (working):**
- `JavaMatch` / `JavaCheckResult::from_json` types with JSON deserialization.
- `compare_results()` and `compare_json_results()` fuzzy matching.
- `ComparisonStatus` enum: Pass, OffsetMismatch, LengthMismatch, ReplacementMismatch, etc.
- `CompatReport` with pass/fail counts and text formatting.
- 9 unit tests.

## Gaps

- Build broken in this checkout: 14 `include_str!` errors in og-langs + hardcoded resource probes; og-spell test paths hardcode `/home/agent/...`. 477 test functions exist, 0 can run here. THE PROJECT ONLY BUILDS ON THE LAYOUT IT WAS DEVELOPED IN.
- No compatibility harness end-to-end: og-compat cannot run Java LT (no subprocess) or the Rust server (no HTTP client; depends on og-api but never uses it). Parity is asserted, never measured. `MessageMismatch` and `CategoryMismatch` declared but never checked; `og-api` and `chrono` deps unused.
- No golden test files or fixture directories exist; `GoldenFixture` is dead code.
- No CI/CD pipeline (no `.github/`), no clippy/rustfmt configuration.
- No XML schema validation and no rule ID uniqueness checks in the test runner; no CLI binary; no multi-file test orchestration.
- Many tests use hardcoded absolute paths.

## Next steps

- Fix the resource root (PLAN.md blocker #1) so the 477 tests can run; replace all hardcoded absolute paths with the configurable root.
- Build the og-compat harness: subprocess/HTTP invocation of Java LT + the Rust server, fixture-driven comparison, diff reports.
- Create the fixtures/ layout and generate expected_java.json outputs at the frozen LT commit.
- Add CI that runs `cargo test --workspace` and (later) parity gates; add clippy/rustfmt config.
- Produce LANGUAGE_MODULES.md and TEST_PORT_LIST.md or fold them into the living matrix.

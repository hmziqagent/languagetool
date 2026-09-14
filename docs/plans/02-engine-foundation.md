# Engine Foundation: Core Text Model & Tokenizer

| | |
|---|---|
| Phases covered | 2 (core text model), 3 (tokenizer/sentence splitter) |
| Status | Phase 2: 80% · Phase 3: 75% |
| Last verified | 2026-09-14 |

## Requirements

### Phase 2 — Core text model

```txt
Document
Sentence
Token
AnalyzedSentence
AnalyzedTokenReadings
RuleMatch
offset model
```

Deliverable: can represent the same data model as the Java engine.

### Phase 3 — Tokenizer and sentence splitter

```txt
Port sentence splitting behavior.
Port tokenization behavior.
Preserve exact offsets.
Add tests.
```

Deliverable: tokenizer/sentence tests pass against Java fixtures.

## Current state

Phase 2: all 17 planned types exist; the checker pipeline works end-to-end. Per-type detail in the og-core table below.

Phase 3: working sentence splitting and word tokenization.

**og-core type-by-type gap table (17 types):**

| Type | Status | Gaps |
|------|--------|------|
| `Language` | Present (enum: 50 variants incl. `Other(String)`; `all_languages()` returns 49) | Missing: country info, CJK flags, tokenizer/tagger references |
| `Document` | Present | Missing: annotations, coordinate mapping |
| `Sentence` | Present | Missing: whitespace tracking |
| `Token` | Present | Missing: type enum (WORD/WHITESPACE/PUNCT), `whitespace_before` |
| `AnalyzedToken` | Present | Missing: `isSentStart/End`, `isImmunized`, chunk tags inline |
| `AnalyzedTokenReadings` | Present (has `is_sentence_start()`/`is_sentence_end()`, `all_pos_tags()`, `all_lemmas()`) | Missing: `getDismissed`, lazy POS lookup |
| `AnalyzedSentence` | Present | Missing: `getAnalyzedTokensWithWhitespace`, language reference |
| `Rule` (trait) | Present | Missing: `reset()`, `activate/deactivate`, config support, priority |
| `TextLevelRule` (trait) | Present | Signature uses `&str` + `&[AnalyzedSentence]`, not `AnalyzedDocument` |
| `RuleMatch` | Present (`type` serialized in API output — see [09-api-parity.md](09-api-parity.md)) | Missing: line/column, inverted suggestions, immunization |
| `SuggestedReplacement` | Present | Missing: type, priority, custom info |
| `Category` | Present | Missing: priority, severity, hierarchy, hidden/remote flags |
| `IssueType` | Present (21 variants) | Comprehensive |
| `Checker` | Present | Missing: caching, auto-detection, thread pool (max text length enforced at og-api layer instead) |
| `CheckRequest` | Present | `level`, `picky`, `mother_tongue` are dead fields (regression — see [09-api-parity.md](09-api-parity.md)) |
| `SentenceRange` | Present | Adequate |

Architectural issues:
- No `AnalyzedDocument` type exists (plan mentions it for `TextLevelRule`).
- No `RuleContext` parameter (plan specifies it in trait signatures).
- Former "two incompatible Tagger traits" issue is resolved: `EnglishTagger` now directly implements `og_core::checker::Tagger` (bridge adapter eliminated).

**og-tokenizer:**

Sentence splitter: 200+ hardcoded abbreviations (word-boundary matched), punctuation/whitespace heuristics, quotation/parenthesis skipping, end-of-text handling. Ellipsis is handled. No SRX.

Word tokenizer: contraction splitting LT-style (20+ contraction stems, 6 suffix patterns, 3 prefix patterns), character-class tokenization, byte-level offset preservation. URL/email detection implemented as atomic tokens (http(s)://, ftp://, www., email — word_tokenizer.rs:17-48).

Abbreviation coverage (200+ vs LT's hundreds):

| Category | OG Count | LT Equivalent |
|----------|----------|---------------|
| Titles | 15+ | 15+ |
| Business | 10+ | 10+ |
| Latin | 15+ | 15+ |
| Months | 12 | 12 |
| Days | 7 | 7 |
| Academic degrees | 10+ | 10+ |
| Publication abbreviations | 10+ | 30+ |
| State/city abbreviations | 20+ | 50+ |
| Word-stem abbreviations | 100+ | 200+ |
| URL/email patterns | Implemented (http(s)://, ftp://, www., email) | regex-based |

## Gaps

- No SRX file loading — LT's sentence splitting is driven by ~50 SRX regex rules; still the #1 source of edge-case sentence-boundary mismatches.
- Hyphen is NOT a word char: "well-known" splits into separate tokens.
- Decimals and "1,000" split into separate tokens by the word tokenizer.
- No numbered list detection.
- No non-breaking space handling.
- Tokenizer not tested against Java fixtures.
- Core type gaps as listed in the table above (token type enum, `whitespace_before`, `AnalyzedDocument`, `RuleContext`, rule lifecycle/activation, document annotations/coordinate mapping).

## Next steps

- Port SRX rule loading for sentence splitting.
- Decide hyphen/number tokenization behavior to match LT exactly.
- Fill core type gaps: token type enum, `whitespace_before`, `AnalyzedDocument`, `RuleContext`, `Rule` lifecycle methods.
- Run tokenizer/sentence tests against Java fixtures (blocked on resource-root fix — see PLAN.md blocker #1).

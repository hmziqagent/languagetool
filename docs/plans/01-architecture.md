# Architecture & Workspace

| | |
|---|---|
| Phases covered | Cross-cutting (rewrite scope, workspace layout, migration strategy) |
| Status | Reference — no % of its own |
| Last verified | 2026-09-14 |

## Requirements

### Rewrite scope

The compute path being rewritten:

```txt
/v2/check request
  ↓
language selection
  ↓
text normalization / markup handling
  ↓
sentence splitting
  ↓
tokenization
  ↓
part-of-speech / morphology analysis
  ↓
disambiguation
  ↓
XML pattern rules
  ↓
native rule classes
  ↓
spellchecking
  ↓
suggestions
  ↓
RuleMatch output
  ↓
LanguageTool-compatible JSON
```

Not rewritten:

```txt
LibreOffice integration
desktop GUI
browser extension
website
Java embedding API
old integrations
non-checking product code
```

LanguageTool's repo README describes it as proofreading software for English, Spanish, French, German, Portuguese, Polish, Dutch, and 20+ other languages — "all languages" means committing to a language-module rewrite, not just an English engine. The API target is mainly `/v2/check`; LT's public HTTP API docs describe that as the proofreading endpoint. LT's development docs confirm the internal model: XML `grammar.xml` rules plus Java `Rule.match(AnalyzedSentence)` rules for logic that cannot be expressed in XML. See Source citations below.

### Workspace layout

```txt
opengrammar-rs/
  crates/
    og-core/
    og-api/
    og-xml/
    og-rules/
    og-spell/
    og-tokenizer/
    og-tagger/
    og-languagemodel/
    og-langs/
    og-test-runner/
    og-compat/
```

Per-crate roles:

**og-core** — core shared types, replacing LT's `JLanguageTool`, `Rule`, `RuleMatch`, `AnalyzedSentence`, `AnalyzedTokenReadings` abstractions:

```txt
Language
Document
Sentence
Token
AnalyzedToken
AnalyzedTokenReadings
AnalyzedSentence
Rule
TextLevelRule
RuleMatch
SuggestedReplacement
Category
IssueType
```

**og-api** — Rust HTTP server (`POST /v2/check`, `GET /v2/languages`, `GET /v2/version` planned; 5 routes now live — see [09-api-parity.md](09-api-parity.md)). Keep `/v2/check` response compatible with LanguageTool.

**og-xml** — LanguageTool XML parser and compiler. Handles:

```txt
grammar.xml
style.xml
disambiguation.xml
grammar_custom.xml
categories
rulegroups
rules
patterns
tokens
exceptions
antipatterns
suggestions
examples
regexp
case_sensitive
skip
min/max
inflected
postags
```

LT supports `grammar_custom.xml` alongside `grammar.xml`, same XML syntax, non-conflicting rule IDs — the loader should support that too.

**og-rules** — native Rust equivalent of Java rule classes (`SpellingCheckRule`, `TextLevelRule`, `PatternRule`, language-specific Rule subclasses). LT docs: rules that cannot be expressed in XML extend Java `Rule` / `match(AnalyzedSentence)`, or `TextLevelRule` for non-sentence-level rules. Rust equivalents:

```rust
pub trait Rule: Send + Sync {
    fn id(&self) -> &str;
    fn match_sentence(&self, sentence: &AnalyzedSentence, ctx: &RuleContext) -> Vec<RuleMatch>;
}

pub trait TextLevelRule: Send + Sync {
    fn id(&self) -> &str;
    fn match_text(&self, doc: &AnalyzedDocument, ctx: &RuleContext) -> Vec<RuleMatch>;
}
```

**og-spell** — dictionary loading, ignore words, compound handling, suggestions, word frequency ranking, language-specific spelling rules.

**og-tokenizer** — sentence boundary detection, word tokenization, punctuation tokenization, URL/email detection, abbreviation handling, offset preservation. (LT's own technical description: take plain text, split into sentences, split sentences into words, find POS tags/base forms, match analyzed sentences against patterns and Java rules.)

**og-tagger** — lemma, part-of-speech tags, readings, base forms, language-specific analyzers, disambiguation support. One of the hardest parts for full compatibility.

**og-languagemodel** — n-gram/language-model compatibility for LT features that use language models. Do not add AI; traditional statistical n-gram features only.

**og-langs** — language modules (`en/`, `de/`, `fr/`, `es/`, `pt/`, `pl/`, `nl/`, ...). Per-module contents: see [06-languages.md](06-languages.md).

**og-test-runner** — Rust equivalent of LT's rule test runner (`testrules.sh`/`testrules.bat`, IDE/JUnit, `mvn clean test`).

**og-compat** — compatibility tooling: run Java LT and Rust OG on same input, compare JSON responses / matches / offsets / replacements / rule IDs, generate diff reports.

Current workspace sizes (2026-09-14):

| Crate | Lines | Status |
|-------|-------|--------|
| og-tagger | 4,474 | Partial (English only) |
| og-rules | 4,046 | Functional |
| og-xml | 4,037 | Mostly complete |
| og-langs | 3,706 | English only; build broken (include_str! paths) |
| og-tokenizer | 2,435 | Mostly complete (no SRX) |
| og-test-runner | 1,957 | Partial |
| og-core | 1,601 | Functional skeleton; 0 tests |
| og-spell | 474 | Minimal |
| og-compat | 444 | Comparison types only |
| og-api | 441 | Working; param wiring regressed |
| og-languagemodel | 26 | Stub |
| **Total** | **23,641** | 11 crates |

### Migration strategy

Compatibility-driven migration — do not manually rewrite random files blindly.

```txt
Step 1:
Freeze one LanguageTool upstream commit.

Step 2:
Vendor/copy required resource files:
- grammar.xml
- style.xml
- disambiguation.xml
- dictionaries
- test files
- example sentences

Step 3:
Build Rust engine skeleton.

Step 4:
Make Rust /v2/check return LT-shaped JSON.

Step 5:
Port XML parser.

Step 6:
Port tokenizer/sentence splitter.

Step 7:
Port PatternRule engine.

Step 8:
Port spellcheck.

Step 9:
Port taggers/morphology.

Step 10:
Port Java Rule classes language by language.

Step 11:
Run same test suite until parity.
```

### Hardest parts

The hard parts are not Axum or XML parsing:

```txt
exact offset compatibility
sentence splitting parity
tokenization parity
POS/morphology parity
disambiguation
spellchecker suggestion ranking
language-specific Java rules
compound-word languages like German
rules that depend on Java class behavior
test suite parity across 20+ languages
```

## Current state

Dependency architecture:

```
og-core (leaf, no internal deps)
  ↑
og-xml, og-tokenizer, og-tagger, og-spell, og-languagemodel (mid-level)
  ↑
og-rules, og-langs (composition)
  ↑
og-api, og-test-runner, og-compat (top-level integration)
```

Infrastructure notes:

```txt
Key dependencies: serde, regex, quick-xml, axum, tokio, rayon, phf (tagger only)
Edition: Rust 2024, MSRV 1.95
License: LGPL-2.1 (declared in Cargo.toml only — no LICENSE file)
No .github/ CI, no clippy/rustfmt config, no repo-root README
```

## Gaps

- No `AnalyzedDocument` type exists (plan specifies it for `TextLevelRule`).
- No `RuleContext` parameter (plan specifies it in trait signatures).
- No LICENSE file, no repo-root README, no CI, no clippy/rustfmt config.
- Upstream LanguageTool commit never formally frozen (migration strategy Step 1).

## Next steps

- Freeze the upstream LT commit and vendor/copy resources into a configurable location (unblocks the build — see PLAN.md blocker #1).
- Introduce `AnalyzedDocument` and `RuleContext` to match the planned trait signatures.
- Add LICENSE file, root README, clippy/rustfmt config, CI skeleton.

## Source citations

1. LT Public HTTP Proofreading API — https://dev.languagetool.org/public-http-api.html (API target is `/v2/check`)
2. LT Development Overview — https://dev.languagetool.org/development-overview.html (XML `grammar.xml` rules + Java `Rule.match(AnalyzedSentence)`)
3. languagetool-org/languagetool README — https://github.com/languagetool-org/languagetool (language-module commitment)
4. JLanguageTool.java — https://github.com/languagetool-org/languagetool/blob/master/languagetool-core/src/main/java/org/languagetool/JLanguageTool.java (core abstractions to replace)
5. LT Tips and Tricks — https://dev.languagetool.org/tips-and-tricks.html (`grammar_custom.xml` support)
6. Daniel Naber, FOSDEM 2014 — https://www.danielnaber.de/publications/fosdem2014.pdf (pipeline description)
7. testrules docs — https://github.com/languagetool-org/languagetool-org.github.io/blob/master/tips-and-tricks.md (rule test runner)
8. LT issue #10327 — https://github.com/languagetool-org/languagetool/issues/10327 (Maven build/test log; one English run loaded 6117 rules)

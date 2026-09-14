# Native Rule Ports

| | |
|---|---|
| Phases covered | 9 (native rule ports) |
| Status | 65% |
| Last verified | 2026-09-14 |

## Requirements

### Phase 9 — Native rule ports

```txt
Port Java Rule classes to Rust.
Port TextLevelRule classes.
Port RuleFilter-like hooks.
```

Deliverable: native rule parity per language.

### Porting strategy

Every Java rule becomes one of:

```txt
1. Already expressible in XML → convert to XML if safe
2. Native Rust rule → direct port
3. Language-specific Rust rule → lives in og-langs/<lang>
4. Unsupported initially → tracked in compatibility matrix
```

Tracking matrix columns (per the plan spec — the current matrix in [compatibility-matrix.md](compatibility-matrix.md) tracks Rule / Rust equivalent / Status):

```txt
Rule ID | Java class | Language | Rust status | Tests passing | Notes
```

Status vocabulary:

```txt
not_started
stubbed
ported
parity_pass
parity_fail
intentionally_skipped
```

## Current state

13 rule structs in `crates/og-langs/src/en/`, ALL wired into `LanguageEngine::english()` (engine.rs:88-263):

| English rule struct | Notes |
|---------------------|-------|
| AvsAnRule | |
| CompoundRule | |
| ContractionSpellingRule | contractions.txt (179 entries) |
| ConsistentApostrophesRule | also wired at text level |
| DashRule | |
| DiacriticsRule | diacritics.txt (1,220 entries; 1,415 lines incl. comments) |
| PlainEnglishRule | |
| SimpleReplaceRule | 6 instances: replace, redundancies, profanity, en-US, en-GB, en-NZ |
| SpecificCaseRule | specific_case.txt (5,540 entries; 5,621 lines incl. comments) |
| StyleFrequencyRule | 3 instances: adjective/noun/verb (also wired as text-level) |
| WordCoherencyRule | also wired as text-level |
| EnglishWordRepeatRule | dedicated port; the generic `WordRepeatRule` is NOT wired (English variant used instead) |
| WrongWordInContextRule | uses its own regex-based context scoring, NOT og-languagemodel |

Also wired into the English engine:

```txt
SpellingCheckRule (if dictionaries load)
XML grammar.xml pattern rules (PatternRuleEngine)
XmlDisambiguator
```

Wired generic sentence rules (from og-rules): DoublePunctuationRule, UppercaseSentenceStartRule, CommaWhitespaceRule.

Wired text-level rules: MultipleWhitespaceRule, SentenceWhitespaceRule, GenericUnpairedBracketsRule, GenericUnpairedQuotesRule, LongSentenceRule (threshold 30), WordRepeatBeginningRule, ConsistentApostrophesRule, WordCoherencyRule, StyleFrequencyRule ×3.

Generic native rules (og-rules) with wired status:

| Rule | Category | Tests | Wired into engine |
|------|----------|-------|-------------------|
| `WordRepeatRule` | MISC/Redundancy | 5 | No — English variant used instead |
| `DoublePunctuationRule` | PUNCTUATION | 3 | Yes |
| `UppercaseSentenceStartRule` | CASING/Capitalization | 5 | Yes |
| `CommaWhitespaceRule` | TYPOGRAPHY/Whitespace | 4 | Yes |

Generic text-level rules (og-rules):

| Rule | Category | Tests | Wired into engine |
|------|----------|-------|-------------------|
| `MultipleWhitespaceRule` | TYPOGRAPHY | 3 | Yes |
| `SentenceWhitespaceRule` | TYPOGRAPHY | 3 | Yes |
| `GenericUnpairedBracketsRule` | PUNCTUATION | 4 | Yes (covers EnglishUnpairedBracketsRule) |
| `GenericUnpairedQuotesRule` | PUNCTUATION | 3 | Yes (covers EnglishUnpairedQuotesRule) |
| `LongSentenceRule` | STYLE | 1 | Yes (threshold 30) |
| `WordRepeatBeginningRule` | STYLE | 2 | Yes |

Pattern-engine filters with real logic (rest are stubs — full table in [03-xml-rule-engine.md](03-xml-rule-engine.md)): FutureDateFilter, DateCheckFilter, NewYearDateFilter/YMDNewYearDateFilter, DateRangeChecker, RegexAntiPatternFilter, ApostropheTypeFilter.

The full English Java Rules Port Status matrix (37 Java classes, updated 2026-09-14: WrongWordInContextRule now Ported; EnglishWordRepeatRule ported as a dedicated rule) lives in [compatibility-matrix.md](compatibility-matrix.md) — update it there, not here.

## Gaps

- 18 of 37 Java rule classes unported, across 13 'Not started' rows in the port matrix (the '+ 6 regional spellers' row covers 6 classes; see [compatibility-matrix.md](compatibility-matrix.md)): EnglishRepeatedWordsRule (picky mode; needs synonyms.txt + synthesizer), MorfologikBritishSpellerRule + 6 regional spellers, EnglishMultitokenSpeller, UnitConversionRule, EnglishConfusionProbabilityRule, EnglishNgramProbabilityRule + UpperCaseNgramRule (need og-languagemodel), 5 false-friend rules.
- The tracking matrix does not yet carry the planned "Tests passing" / "Language" columns; no per-rule parity tests exist (statuses are `ported`, not `parity_pass`).
- English only — no native rule ports for any other language.

## Next steps

- Port the remaining 18 unported rule classes (n-gram rules blocked on og-languagemodel).
- Add per-rule parity tests and upgrade matrix statuses from `ported` to `parity_pass`/`parity_fail`.
- Extend the matrix with the planned Tests-passing/Language columns.

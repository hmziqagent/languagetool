# Spelling, Morphology & Language Model

| | |
|---|---|
| Phases covered | 6 (spellchecker), 7 (POS/morphology/tagger), og-languagemodel |
| Status | Phase 6: 50% · Phase 7: 65% |
| Last verified | 2026-09-14 |

## Requirements

### Phase 6 — Spellchecker

```txt
Load dictionaries.
Detect unknown words.
Generate suggestions.
Rank suggestions.
Port spelling rule behavior.
```

Deliverable: SPELLING_RULE parity for English basic fixtures.

### Phase 7 — POS/morphology/tagging

```txt
Load tagger dictionaries.
Produce lemma/POS readings.
Support disambiguation inputs.
```

Deliverable: AnalyzedTokenReadings parity for selected fixtures.

## Current state

**og-spell:**

| Feature | Status |
|---------|--------|
| Dictionary (HashSet) | Working — plain word lists, case-insensitive |
| `from_file()` / `from_words()` | Working |
| Ignore words | Working |
| `SpellingCheckRule` (implements Rule trait) | Working — wired into the English engine (active if dictionaries load) |
| Suggestions | Brute-force Levenshtein, distance ≤ 3, length-filtered |
| Case preservation in suggestions | Working (capitalized, ALL CAPS) |
| URL/email/digit word skipping | Working (heuristics) |

**og-tagger (EnglishTagger):**

| Feature | Status |
|---------|--------|
| Built-in POS dictionary | ~2,000 words across all Penn Treebank tags |
| External FSA dictionary loading (`dict_decoded.txt`) | Working (~259K lines on the original dev layout; file not present in this checkout — unverified here; loaded at runtime by the engine) |
| `added.txt` loading | Working (engine.rs:92-118) |
| `uncountable.txt` / `partlycountable.txt` loading | Working (engine.rs:92-118) |
| Heuristic unknown-word tagging | Working (15+ suffix rules) |
| Irregular verb lemmatization | Working (~200 forms) |
| Irregular adjective lemmatization | Working (better/best, worse/worst, etc.) |
| Morphological lemma guessing | Working (NNS, VBZ, VBD, VBG, JJR, JJS suffix stripping) |
| VBP expansion from VB | Working |
| Modal-noun disambiguation | Working |
| Penn Treebank + LT extension tags | Full coverage (CC..WRB + NN:U, ORD, SENT_START, etc.) |

Note: all runtime tagger data loading happens in the English engine (engine.rs:92-118) and is currently broken by the resource-path issue (see [06-languages.md](06-languages.md) and PLAN.md blocker #1).

**Chunker:** rule-based, produces NP/VP/PP/ADVP/ADJP/SBAR/PRT tags; used by the tagger and the test runner.

**XmlDisambiguator:** see [03-xml-rule-engine.md](03-xml-rule-engine.md) (actions setPos/replace/remove/add/filter/filterAll/ignoreSpelling; unify is a no-op).

**og-languagemodel:** stub — 26 lines, empty `LanguageModel` struct, `get_probability()` returns 0.0, no data storage, no file loading. Nothing in the workspace depends on it. Needed by `EnglishNgramProbabilityRule` and `UpperCaseNgramRule` (see [05-native-rules.md](05-native-rules.md) and the port matrix in [compatibility-matrix.md](compatibility-matrix.md)). Traditional statistical n-gram features only — no AI.

## Gaps

- No Hunspell `.dic`/`.aff` parsing — only plain word lists, not real dictionaries with morphological rules.
- No compound word handling in spelling.
- No word frequency ranking of suggestions; no phonetic matching (Soundex, Metaphone).
- O(N*M) suggestion performance — scans entire dictionary per word.
- No language-specific spelling rules; URL heuristic is fragile.
- No statistical POS tagger — LT uses trigram HMM disambiguation before XML rules; OG relies on dictionary + heuristics + XML disambiguation, producing more ambiguous output.
- No auto-loading of external dictionaries at `EnglishTagger::new()` (loading is engine-side and path-broken).
- Possible IRREGULAR_FORMS bug (`running => go`).
- No morphological synthesis; unification not implemented; English only.
- og-languagemodel is 0% functional.

## Next steps

- Fix resource root so tagger/spelling dictionaries actually load (PLAN.md blocker #1).
- Add Hunspell `.dic`/`.aff` parsing; compound handling; frequency-based suggestion ranking.
- Decide on the statistical tagger: port an HMM equivalent or document the divergence in the compatibility matrix.
- Fix the IRREGULAR_FORMS bug; add parity tests for tagger output.
- Implement og-languagemodel (n-gram storage + probabilities) or explicitly descope the n-gram rules in the matrix.

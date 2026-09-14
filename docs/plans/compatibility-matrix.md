# Compatibility Matrix (Living Phase 0 Deliverable)

| | |
|---|---|
| Phases covered | Phase 0 deliverable (living document) |
| Status | Partial — English columns filled; other languages pending |
| Last verified | 2026-09-14 |

This is the single source of truth for parity tracking. Update these tables in place; other plans reference them instead of duplicating. English port matrix updated 2026-09-14 (WrongWordInContextRule now Ported; EnglishWordRepeatRule ported as a dedicated rule).

Upstream freeze: the Java LanguageTool reference checkout at `<repo>/languagetool/` is frozen at commit `20b2119977960949c791069b57d191ec1449d211` (recorded 2026-09-14). All Rust resource reads resolve this checkout via the configurable LT resource root (`OG_LT_RESOURCE_ROOT` env var or ancestor search; see `og_spell::resource`).

## Language Modules (37 total, 1 implemented)

| Language | Code | Module Dir | grammar.xml Rules | Status |
|----------|------|-----------|-------------------|--------|
| English | en | en/ | 2,468 category-level entries (1,502 rules + 966 rulegroups); 5,545 total `<rule>` elements | Partially working * |
| German | de | de/ | — | Not started |
| French | fr | fr/ | — | Not started |
| Spanish | es | es/ | — | Not started |
| Portuguese | pt | pt/ | — | Not started |
| Dutch | nl | nl/ | — | Not started |
| Polish | pl | pl/ | — | Not started |
| Russian | ru | ru/ | — | Not started |
| Italian | it | it/ | — | Not started |
| Japanese | ja | ja/ | — | Not started |
| Chinese | zh | zh/ | — | Not started |
| Arabic | ar | ar/ | — | Not started |
| + 25 more | — | — | — | Not started |

\* English engine fully wired (13 native rule structs + generic rules + XML grammar rules + disambiguator — see [05-native-rules.md](05-native-rules.md)), but resource loading is broken in this checkout: see [06-languages.md](06-languages.md) and PLAN.md blocker #1.

grammar.xml counts verified 2026-09-14 by parsing `languagetool/languagetool-language-modules/en/src/main/resources/org/languagetool/rules/en/grammar.xml` (12 categories; 1,502 standalone rules + 966 rulegroups = 2,468 category-level entries; 4,043 rules inside rulegroups; 5,545 total `<rule>` elements). Ids: 1,727 `<rule>` elements carry an id — 1,575 double-quoted + 152 single-quoted, all distinct — and all 966 rulegroups have ids (952 double-quoted + 14 single-quoted), distinct from rule ids; 2,693 unique ids overall. Caution: 152 rule + 14 rulegroup ids use single quotes — double-quote-only parsing undercounts by 166.

## English Java Rules Port Status (37 total)

| Java Rule Class | Rust Equivalent | Status |
|----------------|-----------------|--------|
| AvsAnRule | `og-langs::en::AvsAnRule` | Ported |
| SimpleReplaceRule | `og-langs::en::SimpleReplaceRule` (6 instances: replace, redundancies, profanity, en-US, en-GB, en-NZ) | Ported |
| EnglishWordRepeatRule | `og-langs::en::EnglishWordRepeatRule` (dedicated rule; generic WordRepeatRule not wired) | Ported |
| EnglishRepeatedWordsRule | — | Not started (picky mode, needs synonyms.txt + synthesizer) |
| CompoundRule | `og-langs::en::CompoundRule` | Ported |
| ConsistentApostrophesRule | `og-langs::en::ConsistentApostrophesRule` | Ported |
| EnglishDashRule | `og-langs::en::DashRule` | Ported |
| EnglishSpecificCaseRule | `og-langs::en::SpecificCaseRule` | Ported |
| EnglishUnpairedBracketsRule | `og-rules::text_level_rules::GenericUnpairedBracketsRule` (generic, wired) | Partial |
| EnglishUnpairedQuotesRule | `og-rules::text_level_rules::GenericUnpairedQuotesRule` (generic, wired) | Partial |
| WordCoherencyRule | `og-langs::en::WordCoherencyRule` | Ported |
| MorfologikAmericanSpellerRule | `og-spell::SpellingCheckRule` (generic) | Partial |
| MorfologikBritishSpellerRule | — | Not started |
| + 6 regional spellers | — | Not started |
| EnglishMultitokenSpeller | — | Not started |
| ContractionSpellingRule | `og-langs::en::ContractionSpellingRule` | Ported |
| EnglishDiacriticsRule | `og-langs::en::DiacriticsRule` | Ported |
| LongSentenceRule | `og-rules::text_level_rules::LongSentenceRule` (generic, wired, threshold 30) | Ported |
| StyleTooOftenUsedAdjectiveRule | `og-langs::en::StyleFrequencyRule::adjective()` | Ported |
| StyleTooOftenUsedNounRule | `og-langs::en::StyleFrequencyRule::noun()` | Ported |
| StyleTooOftenUsedVerbRule | `og-langs::en::StyleFrequencyRule::verb()` | Ported |
| EnglishPlainEnglishRule | `og-langs::en::PlainEnglishRule` | Ported |
| EnglishRedundancyRule | `og-langs::en::SimpleReplaceRule::english_redundancies()` | Ported |
| UnitConversionRule | — | Not started |
| EnglishConfusionProbabilityRule | — | Not started |
| EnglishNgramProbabilityRule | — | Not started (needs og-languagemodel) |
| UpperCaseNgramRule | — | Not started (needs og-languagemodel) |
| EnglishWrongWordInContextRule | `og-langs::en::WrongWordInContextRule` (own regex-based context scoring, not og-languagemodel) | Ported |
| EnglishForGermansFalseFriendRule | — | Not started |
| EnglishForFrenchFalseFriendRule | — | Not started |
| EnglishForDutchmenFalseFriendRule | — | Not started |
| EnglishForSpaniardsFalseFriendRule | — | Not started |
| EnglishForL2SpeakersFalseFriendRule | — | Not started |
| AmericanReplaceRule | `og-langs::en::SimpleReplaceRule::american_replace()` | Ported |
| BritishReplaceRule | `og-langs::en::SimpleReplaceRule::british_replace()` | Ported |
| NewZealandReplaceRule | `og-langs::en::SimpleReplaceRule::new_zealand_replace()` | Ported |
| SimpleReplaceProfanityRule | `og-langs::en::SimpleReplaceRule::english_profanity()` | Ported |

Status vocabulary: `not_started`, `stubbed`, `ported`, `parity_pass`, `parity_fail`, `intentionally_skipped` (defined in [05-native-rules.md](05-native-rules.md)). No rule has reached `parity_pass` yet — per-rule parity tests do not exist.

Row accounting: 21 Ported + 3 Partial + 13 'Not started' rows = 37 rows; the 'Not started' rows cover 18 distinct classes (the '+ 6 regional spellers' row expands to 6 classes).

## Core Pipeline Feature Parity

| Feature | Java LT | Rust OG | Status |
|---------|---------|---------|--------|
| Sentence splitting | SRX regex rules (50+ patterns) | Hardcoded 200+ abbreviations + heuristics; ellipsis handled; no SRX | ~60% |
| Word tokenization | SRX + language-specific rules | Contraction splitting + character classes + URL/email atomic tokens | ~70% |
| POS tagging | Morfologik dictionary + HMM | Dictionary + heuristic suffix rules | ~50% |
| Disambiguation | XML rules (1,017 rules, 1,017 disambig actions; comment-stripped count) | XML rules (loaded + executed); unify is a no-op | ~65% |
| Chunking | OpenNLP IOB chunker | Rule-based IOB chunker | ~70% |
| Spellchecking | Morfologik + Hunspell + SymSpell | word-list dict + Levenshtein (deterministic ranking, early-exit DP) + `og-spell::hunspell` (.dic/.aff subset: FLAG ASCII/LONG/NUM, PFX/SFX depth-2 with conditions, COMPOUNDFLAG/BEGIN/MIDDLE/END, ONLYINCOMPOUND — verified against de_AT; no recursive continuation classes, no SymSpell) | ~60% |
| Pattern rules | Full XML grammar.xml (5,545 rule elements) | XML loaded + most features work | ~80% |
| Native rules | 37 Java classes | 4 generic + 13 English structs + SpellingCheckRule (all wired) | ~65% |
| Language model | N-gram probabilities (Lucene-indexed corpora) | Count-based n-gram model in `og-languagemodel` (add-k smoothing + stupid backoff, corpus-trained; Lucene ngram binary index NOT parsed — host supplies a plain-text corpus) | ~40% |
| API | 8 endpoints | 5 endpoints | ~60% |

## API Endpoint Parity

| Endpoint | Java LT | Rust OG | Status |
|----------|---------|---------|--------|
| `POST/GET /v2/check` | Full | Working (only `text`/`data` + `language` acted on; 7 params parsed but ignored — see [09-api-parity.md](09-api-parity.md)) | ~80% |
| `GET /v2/languages` | Full | Working | ~90% |
| `GET /v2/version` | — | Working | Done |
| `GET /v2/maxtextlength` | Yes | Working (MAX_TEXT_LENGTH = 100,000 enforced) | Done |
| `GET /v2/info` | Yes | Working | Done |
| `GET /v2/configinfo` | Yes | Missing | Not started |
| `GET/POST/DELETE /v2/words` | Yes | Missing | Not started |
| `GET /v2/users/me` | Yes | Missing | Not started |

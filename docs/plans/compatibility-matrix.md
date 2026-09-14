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
| MorfologikBritishSpellerRule | — | Not started (per-variant Morfologik binary .dict files absent from the frozen checkout — cannot be reconstructed from the available spelling_en-XX.txt supplements without false positives) |
| + 6 regional spellers | — | Not started (same missing per-variant binary dictionaries) |
| EnglishMultitokenSpeller | — | Not started |
| ContractionSpellingRule | `og-langs::en::ContractionSpellingRule` | Ported |
| EnglishDiacriticsRule | `og-langs::en::DiacriticsRule` | Ported |
| LongSentenceRule | `og-rules::text_level_rules::LongSentenceRule` (generic, wired, threshold 30) | Ported |
| StyleTooOftenUsedAdjectiveRule | `og-langs::en::StyleFrequencyRule::adjective()` | Ported |
| StyleTooOftenUsedNounRule | `og-langs::en::StyleFrequencyRule::noun()` | Ported |
| StyleTooOftenUsedVerbRule | `og-langs::en::StyleFrequencyRule::verb()` | Ported |
| EnglishPlainEnglishRule | `og-langs::en::PlainEnglishRule` | Ported |
| EnglishRedundancyRule | `og-langs::en::SimpleReplaceRule::english_redundancies()` | Ported |
| UnitConversionRule | `og-langs::en::UnitConversionRule` (GENERAL wired; `us()`/`imperial()` variants available, unwired) | Partial (SUGGESTION path only — non-metric units get a metric-equivalent suggestion; Java's CHECK path verifying user-written conversions not ported; picky-only default-off like Java's Tag.picky) |
| EnglishConfusionProbabilityRule | `og-langs::en::EnglishConfusionProbabilityRule` (confusion_sets.txt + shared LM; no-op untrained) | Ported (subset: trigram factor comparison, directional pairs; Java exception phrases/anti-patterns not ported; first/last sentence tokens unscored — Java pads with _START_/_END_) |
| EnglishNgramProbabilityRule | `og-langs::en::EnglishNgramProbabilityRule` (default-off like Java; no-op untrained) | Ported (subset: flags unattested trigrams of in-vocabulary words instead of Java's absolute 1e-14 probability threshold — the count model's add-k smoothing makes every trigram positive; no POS-synthesizer suggestions, only naive singular/plural variants) |
| UpperCaseNgramRule | `og-langs::en::UpperCaseNgramRule` (id EN_UPPER_CASE_NGRAM; no-op untrained) | Ported (subset: the case probability ratio is uncomputable against a case-insensitive model, so an occurrence threshold (>=5) on the lowercase trigram stands in for Java's ratio > 50; Aho-Corasick exception phrases and the spell-check pre-filter skipped; anti-pattern subset) |
| EnglishWrongWordInContextRule | `og-langs::en::WrongWordInContextRule` (own regex-based context scoring, not og-languagemodel) | Ported |
| EnglishForGermansFalseFriendRule | `og-langs::en::FalseFriendRule::for_germans` (confusion_sets_l2_de.txt; self-gates on request motherTongue=de) | Ported (subset of ConfusionProbabilityRule core; no-op when LM untrained) |
| EnglishForFrenchFalseFriendRule | `og-langs::en::FalseFriendRule::for_french` (confusion_sets_l2_fr.txt) | Ported (same subset as above) |
| EnglishForDutchmenFalseFriendRule | `og-langs::en::FalseFriendRule::for_dutch` (confusion_sets_l2_nl.txt) | Ported (same subset as above) |
| EnglishForSpaniardsFalseFriendRule | `og-langs::en::FalseFriendRule::for_spaniards` (confusion_sets_l2_es.txt) | Ported (same subset as above) |
| EnglishForL2SpeakersFalseFriendRule | — | Intentionally skipped (abstract base class in Java — no data of its own; the concrete de/fr/nl/es subclasses are ported; its XML false-friends.xml pattern-rule half is not ported) |
| AmericanReplaceRule | `og-langs::en::SimpleReplaceRule::american_replace()` | Ported |
| BritishReplaceRule | `og-langs::en::SimpleReplaceRule::british_replace()` | Ported |
| NewZealandReplaceRule | `og-langs::en::SimpleReplaceRule::new_zealand_replace()` | Ported |
| SimpleReplaceProfanityRule | `og-langs::en::SimpleReplaceRule::english_profanity()` | Ported |

Status vocabulary: `not_started`, `stubbed`, `ported`, `parity_pass`, `parity_fail`, `intentionally_skipped` (defined in [05-native-rules.md](05-native-rules.md)). No rule has reached `parity_pass` yet — per-rule parity tests do not exist.

Row accounting (updated 2026-09-14, area 7): 28 Ported + 4 Partial + 1 intentionally_skipped + 4 'Not started' rows = 37 rows; the 'Not started' rows cover 8 distinct classes (EnglishRepeatedWordsRule, MorfologikBritishSpellerRule, + 6 regional spellers, EnglishMultitokenSpeller).

Default-on verification (2026-09-14): `WORD_REPEAT_BEGINNING_RULE` default-ON in Rust was previously suspected to diverge from upstream — verified against the frozen Java checkout: `English.java:189` wires `EnglishWordRepeatBeginningRule`, neither it nor the `WordRepeatBeginningRule`/`Categories.REPETITIONS_STYLE` chain calls `setDefaultOff`, so the upstream rule is default-ON. No divergence. Known real divergence instead: Rust `LongSentenceRule` threshold is 30, upstream English uses 40 (`English.java` `new LongSentenceRule(messages, userConfig, 40)`).

The n-gram-backed rules (NGRAM_RULE, EN_UPPER_CASE_NGRAM, CONFUSION_RULE, false friends) share one process-wide `og_languagemodel::LanguageModel` trained from the plain-text corpus file named by `OG_LT_NGRAM_CORPUS` (order 3). With no variable set the model is empty and every n-gram rule is a no-op — matching Java, where these rules only exist when an ngram index is configured.

## Core Pipeline Feature Parity

| Feature | Java LT | Rust OG | Status |
|---------|---------|---------|--------|
| Sentence splitting | SRX regex rules (50+ patterns) | Hardcoded 200+ abbreviations + heuristics; ellipsis handled; no SRX | ~60% |
| Word tokenization | SRX + language-specific rules | Contraction splitting + character classes + URL/email atomic tokens | ~70% |
| POS tagging | Morfologik dictionary + HMM | Dictionary + heuristic suffix rules | ~50% |
| Disambiguation | XML rules (1,017 rules, 1,017 disambig actions; comment-stripped count) | XML rules (loaded + executed); unify is a no-op | ~65% |
| Chunking | OpenNLP IOB chunker | Rule-based IOB chunker | ~70% |
| Spellchecking | Morfologik + Hunspell + SymSpell | word-list dict + Levenshtein (deterministic ranking, early-exit DP) + `og-spell::hunspell` (.dic/.aff subset: FLAG ASCII/LONG/NUM, PFX/SFX depth-2 with conditions, COMPOUNDFLAG/BEGIN/MIDDLE/END (stem flags AND affix continuation flags, verified against de_AT incl. positive compound), ONLYINCOMPOUND, FORBIDDENWORD (must-reject), NEEDAFFIX, KEEPCASE, cross-product Y/N permission (unit-tested; de_AT's N groups no longer combine with prefixes); no recursive continuation expansion, COMPOUNDPERMITFLAG not enforced, no SymSpell) | ~60% |
| Pattern rules | Full XML grammar.xml (5,545 rule elements) | XML loaded + most features work | ~80% |
| Native rules | 37 Java classes | 4 generic + 20 English structs (incl. 7 n-gram/false-friend/unit rules, no-ops until OG_LT_NGRAM_CORPUS trains the shared model) + SpellingCheckRule | ~85% |
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

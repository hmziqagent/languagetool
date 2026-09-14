# XML Parser, Pattern Rule Engine & Disambiguation

| | |
|---|---|
| Phases covered | 4 (XML parser), 5 (pattern rule engine), 8 (disambiguation) |
| Status | Phase 4: 85% · Phase 5: 80% · Phase 8: 65% |
| Last verified | 2026-09-14 |

## Requirements

### Phase 4 — XML parser/compiler

```txt
Parse grammar.xml.
Parse style.xml.
Parse examples.
Validate rule IDs.
Compile to Rule AST.
```

Deliverable: can load English grammar.xml without crashing.

### Phase 5 — Pattern rule engine

```txt
Implement token matcher.
Implement regex matching.
Implement exceptions.
Implement antipatterns.
Implement suggestions.
```

Deliverable: simple XML rule tests pass.

### Phase 8 — Disambiguation

```txt
Port disambiguation XML.
Apply disambiguation before grammar rules.
Test POS-sensitive XML rules.
```

Deliverable: POS-dependent pattern rules start passing.

### XML compatibility target (start order)

XML compatibility is not optional — it is central. The engine must eventually support the LT rule language, in this order:

```txt
Level 1:
basic <rule>, <pattern>, <token>, <message>, <suggestion>

Level 2:
regexp, case sensitivity, exceptions, antipatterns

Level 3:
rulegroups, examples, categories, default enable/disable

Level 4:
POS constraints, inflected forms, lemmas, skip/min/max

Level 5:
backreferences, markers, suggestion transforms

Level 6:
disambiguation rules

Level 7:
filter hooks / Java RuleFilter equivalents rewritten in Rust
```

Do not invent another rule format.

## Current state

**og-xml:**
- Loads real English grammar.xml without crashing (verified 2026-09-14 against the LT checkout: 12 categories; 2,468 category-level entries = 1,502 standalone rules + 966 rulegroups; 5,545 total `<rule>` elements compile to pattern rules, 4,043 of them rulegroup sub-rules; 1,727 rules carry an id — 1,575 double-quoted + 152 single-quoted, all distinct — plus 966 distinct rulegroup ids).
- Supported elements: rule, rulegroup, category, pattern, token, and, or, marker, exception, match, message, suggestion, short, url, antipattern.
- NOT supported: unify, phrases, while. style.xml and grammar_custom.xml are never referenced anywhere.
- Parsed and propagated: `<rule description>` / `<rule sub_id>` / `<rule|rulegroup|category type>`, `<url>` content, `<short>` content (as `short_message`), `<example correction="...">` text split on `|`. Marker-based implicit Incorrect detection for examples works.
- `<token>` attributes parsed and compiled: `regexp`, `postag`, `postag_regexp`, `negate`, `negate_pos`, `case_sensitive`, `inflected`, `min`, `max`, `skip`, `spacebefore`, `chunk`, `chunk_re`.

XML compatibility levels:

| Level | Scope | Status |
|-------|-------|--------|
| 1 | Basic `<rule>`, `<pattern>`, `<token>`, `<message>`, `<suggestion>` | Complete |
| 2 | `regexp`, `case_sensitive`, exceptions, antipatterns | Complete |
| 3 | `rulegroups`, examples, categories, default enable/disable | Mostly done |
| 4 | POS constraints, inflected, lemmas, skip/min/max | Mostly done |
| 5 | Backreferences, markers, suggestion transforms | Mostly done |
| 6 | Disambiguation rules | Mostly done (unify parsed but runtime no-op) |
| 7 | Filter hooks | Partial (6 filters implemented, 10+ stubs — see filter table) |

**og-rules pattern engine feature table:**

| Feature | Status |
|---------|--------|
| Literal text matching | Working |
| Regex matching (anchored) | Working |
| `inflected="yes"` (lemma fallback) | Working |
| `negate` / `negate_pos` | Working |
| POS tag constraints | Working |
| Chunk constraints | Working |
| `spacebefore` | Working |
| Exceptions (with scope: next/previous/current) | Working |
| Skip tokens | Working (including unlimited skip) |
| Min/max repetition | Working (greedy) |
| Optional tokens (min=0) | Working |
| OR groups / AND groups | Working |
| Antipatterns (with overlap immunization) | Working |
| Markers (error span) | Working |
| Backreferences in patterns | Working |
| Suggestion generation (text + match refs) | Working |
| Case conversion (startupper/alllower/etc.) | Working |
| Regexp match/replace in suggestions | Working |
| First-token word index | Working |
| Rayon parallelism | Working |
| Compiled regex cache | Working |

`default_on` is honored (rules with `default_on=false` are skipped unless picky or explicitly enabled). `sub_id`, `url`, `short_message`, `issue_type` propagate to RuleMatch (issue_type fallback chain: rule → category → group → "grammar").

**Filter table:**

| Filter | Status |
|--------|--------|
| `FutureDateFilter` | Working (uses current date) |
| `DateCheckFilter` | Working (Zeller's congruence) |
| `NewYearDateFilter` / `YMDNewYearDateFilter` | Working (uses current date) |
| `DateRangeChecker` | Working (checks date range validity) |
| `RegexAntiPatternFilter` | Working (rejects matches overlapping antipattern regex) |
| `ApostropheTypeFilter` | Working (typographic vs typewriter apostrophe) |
| `EnglishSuppressMisspelledSuggestionsFilter` | Stub (reads args) |
| All other filters (10+) | Stub (return true) |

**Disambiguation (XmlDisambiguator):**
- Loads real LT English disambiguation.xml (17,228 lines, verified 2026-09-14; 1,017 rules and 1,017 disambig actions, comment-stripped count).
- Supports: text/regex matching, POS constraints, or/and groups, antipatterns, markers.
- Actions: setPos, replace, remove, add, filter, filterAll, ignoreSpelling.
- Applied before grammar rules in the pipeline.
- `DisambigAction::Unify` is parsed (disambig_parser.rs:203) but is a runtime no-op (xml_disambiguator.rs:534).

## Gaps

- No `<unify>` support in grammar rules (parsed as a no-op); many LT rules use `<unify>` for variable binding across pattern tokens.
- No `<phrases>` / `<while>` / `<tests>` support.
- style.xml and grammar_custom.xml never loaded/referenced.
- No rule ID uniqueness validation.
- Entity expansion is simplistic (no external entities, no parameter entities).
- Most Java filters are still stubs; no `<and>`/`<or>` inside exceptions.
- 5 dead helper functions in og-rules text_level_rules.rs.
- Disambiguation not tested against Java fixtures; English only.

## Next steps

- Implement `<unify>` (feature parsing + variable binding) — biggest parity lever in this area.
- Add `<phrases>`/`<while>`; load style.xml and grammar_custom.xml.
- Implement remaining filters; replace stubs.
- Add rule ID uniqueness validation; improve entity expansion.
- Test disambiguation and pattern rules against Java fixtures (blocked on resource-root fix — see PLAN.md blocker #1).

# Performance

| | |
|---|---|
| Phases covered | 12 (performance) |
| Status | 35% |
| Last verified | 2026-09-14 |

## Requirements

### Phase 12 — Performance

Only after correctness.

```txt
Rule indexing
compiled regex cache
parallel sentence checking
dictionary memory optimization
zero-copy token spans where possible
```

Deliverable: Rust equal or faster than Java on same fixtures.

## Current state

- First-token word index (HashMap) for rule lookup.
- Rayon parallelism for sentence checking.
- Compiled regex cache.
- Length-filtered spellchecker suggestions (bounds the Levenshtein candidate scan).

## Gaps

- No dictionary memory optimization — everything is `HashSet<String>`.
- No zero-copy token spans.
- Spellchecker is O(N*M) per suggestion (scans the entire dictionary per unknown word).
- FSA dictionary uses HashMap instead of a trie/automaton.
- No benchmark suite / fixture timing, so "equal or faster than Java" is unmeasured.

## Next steps

- Add a benchmark harness over the golden fixtures once they exist (see [07-testing-compatibility.md](07-testing-compatibility.md)).
- Replace HashMap-based dictionary structures with a trie/FSA automaton.
- Bound spellchecker suggestion search (frequency-indexed candidates or BK-tree) instead of full-dictionary scans.
- Introduce zero-copy token spans where the offset model allows.

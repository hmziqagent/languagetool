# API Parity

| | |
|---|---|
| Phases covered | 1 (API shell) |
| Status | 80% |
| Last verified | 2026-09-14 |

## Requirements

### Phase 1 — Rust API shell

```txt
Axum server
/v2/check
/v2/languages
LT-compatible response structs
No real checking yet
```

Deliverable: `curl /v2/check` works and returns valid LT-shaped JSON.

### API server parity (plan)

The API should accept LT-style params (LT's public HTTP API docs describe `/v2/check` as the proofreading endpoint — citation [1] in [01-architecture.md](01-architecture.md)):

```txt
text
language
motherTongue
enabledRules
disabledRules
enabledCategories
disabledCategories
level
picky
```

Return LT-style response:

```txt
software
warnings
language
matches
sentenceRanges
extendedSentenceRanges
```

Focus first on:

```txt
text
language
matches
offset
length
replacements
rule
category
```

Then add the rest.

## Current state

- 5 routes live: `GET/POST /v2/check`, `GET /v2/languages`, `GET /v2/version`, `GET /v2/info`, `GET /v2/maxtextlength`. Server listens on port 8081. CORS enabled.
- `MAX_TEXT_LENGTH = 100_000` is enforced on `/v2/check` (also surfaced via `/v2/maxtextlength`).
- What works end-to-end: POST `/v2/check` with English text returns LT-shaped JSON with real grammar rule matches.
- Language handling: "en", "en-US", "en-GB" map to three separate `LanguageEngine::english()` instances; any other language → 400 "Unsupported language".

**Parameter support table (UPDATED 2026-09-14 — REGRESSION):**

| Parameter | Parsed | Acted On |
|-----------|--------|----------|
| `text` | Yes | Yes |
| `language` | Yes | Yes |
| `motherTongue` | Yes | **No** |
| `enabledRules` | Yes | **No** |
| `disabledRules` | Yes | **No** |
| `enabledCategories` | Yes | **No** |
| `disabledCategories` | Yes | **No** |
| `level` | Yes | **No** |
| `picky` | Yes | **No** |

Regression note: `handle_check` (handlers.rs:10-64) now acts on ONLY `text`/`data` and `language`. The seven parameters above are parsed but ignored — the per-language engine refactor (commit c644300) dropped the wiring that commit 1c4dc4c had added. Earlier audit states claiming these params are functional are obsolete.

**Response shape (present):**
- Top-level: `software`, `warnings`, `language`, `matches`, `sentenceRanges`, `extendedSentenceRanges` — all present.
- `language.name`, `language.detected.detectedBy`, `language.detected.confidence` — present.
- `RuleMatch.type` — present (serialized as `"type"`).
- `code`/`longCode` split in languages — `code` returns the short code (e.g. "en"), `longCode` the full code (e.g. "en-US").

## Gaps

- The 7 parsed-but-ignored parameters (see table) — must be re-wired into rule filtering.
- `RuleMatchRule` missing `isPremium`, `sourceFile`, `group`.
- `extendedSentenceRanges` always empty.
- Missing endpoints vs Java LT: `/v2/configinfo`, `/v2/words` (GET/POST/DELETE), `/v2/users/me` (see [compatibility-matrix.md](compatibility-matrix.md)).
- `/v2/check` response still missing some fields vs Java LT.

## Next steps

- Re-wire the 7 regressed parameters in `handle_check` (restore c644300's lost behavior from 1c4dc4c): rule/category enable/disable, level, picky, motherTongue.
- Add `isPremium`/`sourceFile`/`group` to `RuleMatchRule`; populate or intentionally descope `extendedSentenceRanges`.
- Add `/v2/configinfo`, `/v2/words`, `/v2/users/me` per the endpoint parity table.
- Add API-level golden tests (see [07-testing-compatibility.md](07-testing-compatibility.md)).

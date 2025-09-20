# M3U Proxy Remediation & Hardening Plan
_Status: living document. Update as tracks complete._

## Guiding Principles
- Incremental, low‑risk changes; ship early, verify with tests and logging, then iterate.
- Every code change MUST land with at least one new or updated test (unit / integration / e2e).
- Prefer enabling observability (structured logs, small samples) over guesswork.
- Preserve backward compatibility for existing expressions unless explicitly versioned.

## Legend
- [ ] Not started
- [~] In progress
- [x] Complete
- (O) Optional / stretch / nice-to-have
- (⚠) Requires follow-up discussion / design

---

## Track 1: Filter Alias Support & Correct Matching (CURRENT FOCUS)

| Goal | Ensure EPG filter expressions properly recognize alias fields (e.g. `program_title`) and match expected records (e.g. `channel_id contains "sport"`). |
|------|------------------------------------------------------------------------------------------------------------------------------|

### Tasks
- [x] Add alias map to `EpgFilterProcessor` parser pipeline (`with_aliases`).
- [x] Switch filter parsing to `.parse_extended()` for parity with rule processors.
- [x] Add structured trace logging of: filter_id, canonicalized fields, node count.
- [x] Unit test: alias resolution (`program_title contains "Match"` ⇒ canonical `programme_title`).
- [x] Unit test: substring “sport” matches sample program set.
- [x] Integration test (EPG): fabricate 3–5 programs; filter returns correct subset.
- [x] Regression test: unknown alias produces validation error + suggestion (if similarity logic applies).
- [x] Documentation snippet: alias behavior & canonical field naming.

### Acceptance Criteria
- Filter UI accepts both canonical + alias names without marking them invalid. (Pending front-end validation wiring)
- Backend logs show non-zero nodes for the “sport” expression. (Met)
- Tests pass (CI green). (Met for implemented backend scope)

---

## Track 2: Data Mapping Preview & Execution Consistency

| Goal | Align preview endpoint parsing & execution with runtime rule engine so preview counts reflect actual effects. |

### Tasks
- [ ] Audit preview endpoints to ensure same field list + alias map as `EpgRuleProcessor`.
- [ ] Add debug logging (guarded by feature flag `debug-expressions`): raw → canonical expression, node count, sample matched IDs.
- [ ] Integration test: `(channel_id contains "sports") SET programme_category ?= "Sports"` yields >0 condition matches with fixture data.
- [ ] Unit test: `?=` (assign-if-null) does not overwrite non-null categories.
- [ ] Provide at least 1 preview sample row (first N) when `condition_matches == 0` to reduce false negative confusion (O).

### Acceptance Criteria
- Preview returns non-zero matches for known dataset.
- Actual application run updates expected number of rows (within test fixture).

---

## Track 3: Missed Ingests for Never-Run Sources

| Goal | Ensure newly created sources with cron schedules that would have already fired are ingested immediately on startup if configured. |

### Tasks
- [ ] Extend `log_startup_schedule()` to treat `last_ingested_at == NULL` as pending when first scheduled time <= now (guarded by config flag).
- [ ] Add config flag: `ingestion.initial_run_on_start` (default: true) OR reuse `run_missed_immediately` semantics.
- [ ] Unit/integration test: new source triggers immediate ingest; when flag disabled it does not.
- [ ] Logging: `[SCHED_INIT] scheduling initial run source_id=... reason=never_ingested`.

### Acceptance Criteria
- Startup logs show scheduling of never-ingested sources.
- Test demonstrates conditional behavior with flag toggle.

---

## Track 4: Implement `case_sensitive` Modifier

| Goal | Make the parsed `case_sensitive` flag effective; preserve current default case-insensitive semantics. |

### Tasks
- [ ] Branch evaluation for Equals / NotEquals / Contains / NotContains / StartsWith / EndsWith in both filter + rule processors.
- [ ] Unit tests:
  - Case-insensitive default still matches.
  - With `case_sensitive` modifier mismatch occurs when casing differs.
- [ ] README section: Expression modifiers.

### Acceptance Criteria
- Tests verify behavioral divergence only when modifier present.
- No change to existing expressions (backward compatible).

---

## Track 5: Theme “Flashbomb” Elimination (White Flash)

| Goal | Remove white flash during navigation; dark theme applied before first paint. |

### Tasks
- [ ] Add inline bootstrap script (pre-bundle) to set `<html class="dark">` or stored theme.
- [ ] Ensure SSR provides fallback dark class if no preference.
- [ ] Add `color-scheme: dark;` to base CSS when dark mode active.
- [ ] Playwright visual regression / performance trace to confirm absence of white frame.
- [ ] (O) Add fade transition only after theme class has been applied.

### Acceptance Criteria
- No white flash in manual & automated (screenshot) testing.
- Lighthouse / performance unaffected (or improved).

---

## Track 6: Debug Page Stale “Size / Age” Fields

| Goal | Remove or conditionally display deprecated cache config fields; reflect real runtime capabilities. |

### Tasks
- [ ] Make display conditional: only render if JSON includes `max_size_mb` or `max_age_days`.
- [ ] Update TS types: mark deprecated properties optional or removed.
- [ ] Add "Capabilities" / "Schema version" block to clarify active features.
- [ ] Unit test: component renders gracefully without legacy fields.
- [ ] Playwright snapshot test updated.

### Acceptance Criteria
- Debug page no longer shows misleading fields.
- Tests cover absence scenario.

---

## Track 7: Consolidated Test Enhancements

| Goal | Ensure broad confidence via automated suite covering new expression & ingestion behaviors. |

### Tasks
- [ ] New backend test module for alias + case-sensitive scenarios.
- [ ] Add numeric comparison unit tests (<, <=, >, >=) including mixed string→number coercion & invalid numeric literal errors (O).
- [ ] Data mapping preview integration test (sports category assignment).
- [ ] Scheduler initial-run test (mock or controlled cron).
- [ ] Playwright:
  - Expression editor alias validation.
  - Data mapping preview non-zero matches.
  - Theme navigation (no flash).
  - Debug page regressions.
- [ ] Add CI grouping so failures point to track (naming convention: `trackX_*`).

### Acceptance Criteria
- All new tests pass consistently in CI.
- Failures clearly attributable to specific track via naming.

---

## Track 8: Observability & Diagnostics (Supporting)

| Goal | Improve root cause discovery without excessive log noise. |

### Tasks
- [ ] Add structured logs:
  - `[EXPR_PARSE] {id,type,fields,node_count}`
  - `[EXPR_MATCH] {id,matched,applied,sample_ids=[...]}` (sample limited to e.g. 3).
- [ ] Feature flag gating (config: `features.flags.debug-expressions`).
- [ ] Add `/api/v1/diagnostics/expression/parse` (O) endpoint returning parsed tree (secured/admin only).
- [ ] Document how to enable & interpret logs.

### Acceptance Criteria
- Debug flag off: no noisy logs.
- Debug flag on: logs contain actionable parse data.

---

## Track 9: Documentation Updates

| Goal | Keep developer + user docs in sync with new behaviors. |

### Tasks
- [ ] Update README (Expressions section) for:
  - Alias canonicalization.
  - `?=` semantics.
  - `case_sensitive` modifier.
  - Preview vs execution parity guarantees.
- [ ] Add troubleshooting section (“Why does my filter match 0?”).
- [ ] Include link to diagnostics enabling.

### Acceptance Criteria
- Docs merged concurrent with feature completion.
- Internal team sign-off (self-review or PR checklist entry).

---

## Risk & Mitigation Summary

| Risk | Mitigation |
|------|------------|
| Bursty initial ingests after enabling Track 3 | Respect concurrency limits already in scheduling config. |
| Performance regression from added logging | Gate logs behind feature flag & appropriate log level. |
| User confusion if alias change broadens matches unexpectedly | Release notes; explicit commit message; test coverage. |
| Theme script CSP issues | If CSP strict, add nonce to inline script or pre-render class via SSR instead. |

---

## Track 10: Performance Benchmarking (Optional)

| Goal | Establish baseline and detect regressions in expression parsing & evaluation under increasing volume. |

### Tasks
- (O) Add Criterion benchmarks for parser (cold) and evaluation (warm with cached field lookups).
- (O) Generate synthetic expressions: simple, medium, complex (nested boolean).
- (O) Benchmark evaluation on datasets: 1k, 10k, 100k program rows (mock or in-memory).
- (O) Add optional cargo feature `bench-expr` to gate heavy benchmark-specific code.
- (O) Define regression threshold: >15% slowdown vs last tagged release fails CI (if benchmarks enabled).
- (O) Consider simple per-run timing log `[EXPR_BENCH] {cases,avg_ms,p95_ms}` outside Criterion for lightweight tracking.

### Acceptance Criteria
- Benchmarks runnable locally via `cargo bench`.
- Documented in README / Track 10 notes how to run & interpret.
- Optional CI job can be toggled to guard performance.

---

## Metrics / Validation Hooks (Optional)
- (O) Add counters: `expr_parser_success_total`, `expr_parser_error_total`, `expr_condition_empty_total`.
- (O) Add histogram: `data_mapping_rule_application_time_ms`.

---

## Completion Checklist (to update as we proceed)

| Track | Status | Notes |
|-------|--------|-------|
| 1 | [x] | Complete: parser, logging, tests, and documentation (alias behavior & canonical field naming) added. |
| 2 | [ ] | Await Track 1 completion. |
| 3 | [ ] | Requires config flag decision. |
| 4 | [ ] | Low risk; can parallelize after Track 1. |
| 5 | [ ] | Frontend only; after critical backend fixes. |
| 6 | [ ] | Simple UI cleanup. |
| 7 | [ ] | Incremental as tracks merge. |
| 8 | [ ] | Logging hooks after Tracks 1–2. |
| 9 | [ ] | Batch doc update before first release containing changes. |
| 10 | [ ] | Optional performance benchmarks & regression guard. |

---

## Immediate Next Actions (Sprint 1)
1. Implement Track 1 code changes.
2. Add/Run unit & integration tests for alias + “sport” matching.
3. Update this file with Track 1 status → [~] / [x].

---

_Keep this document in the repo root for visibility. Link it in contributor guidelines if needed._
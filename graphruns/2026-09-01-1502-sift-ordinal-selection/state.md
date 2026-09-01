# state.md — 2026-09-01-1502-sift-ordinal-selection

Disk is truth; context is cache. Re-read before computing every frontier.
Lifecycle: agent `pending → dispatched → returned → verified | failed`;
transform `pending → verified`; gate `pending → staged → approved | rejected`.
**`returned` ≠ `verified`; only `verified` unlocks dependents.**

## Dispatch tally

- this wave: 1
- cumulative: 10  (planned 8, threshold 12 — not crossed)
- §10 sizing: 8 planned · stop-and-ask threshold **12** (150%)
- IMPL_ROUND: 1 / cap 2  (progress metric: blocking+unmet count, must strictly decrease; baseline 1 blocking) · DOC_ROUND: 0 / cap 2

## Final goal check

`goal-check.sh` post-work → **6 passed, 0 failed, exit 0** (`goal-check-final.log`).
Pre-work it was 2/6 with 4 failures on the missing work. Same script, both ends.

## Negative control (pre-work, §9)

`goal-check.sh` run 2026-09-01 15:07 → **2 passed, 4 failed, exit 1** — FAILS as required,
and fails on the missing ordinal work (checks 3,4,5,6) while checks 1–2 (build clean 0
warnings; jump 13/0) PASS, proving the check is aimed at a working target.
Log: `negative-control.log`.

## Rows

| id | status | attempt | model | bound | dispatched-at | verified-by | result-ref |
|---|---|---|---|---|---|---|---|
| baseline | verified | 0 | transform | 5m | — | build clean 0 warnings; jump 13/0; live 6/0 (measured 2026-09-01 15:0x) | `negative-control.log` checks 1–2 |
| impl-a | verified | 1 | opus→sonnet | 25m | 2026-09-01 15:10 | orchestrator: git diff = main.cpp only (+22/-1); clean rebuild 0 warnings; jump 13/0; live 6/0 — all re-run independently | `nodes/impl-a/result.md` |
| build-a | verified | 0 | transform | 5m | — | `cmake --build --preset release --clean-first` rc 0, 0 warnings; both suites at baseline | `scratchpad/build-a.log` |
| impl-b | verified | 2 | opus→sonnet | 40m | 2026-09-01 15:17 | attempt 2 (footer guard): orchestrator width sweep 8/0 WITH a failing control (5/8 fail unguarded); feature probe 13/0 at w=100 AND w=74; clean rebuild 0 warnings; jump 13/0; live 6/0 | `nodes/impl-b/result.md` |
| build-b | verified | 0 | transform | 5m | — | rc 0, 0 warnings, both suites at baseline | `scratchpad/build-b.log` |
| verify-impl | verified | 1 | opus→sonnet | 25m | 2026-09-01 15:31 | orchestrator reproduced the blocking finding at w=74 (header lost, goto> count 0); 25-item checklist with pasted evidence | `nodes/verify-impl/result.md` |
| atlas | verified | 1 | sonnet→haiku | 20m | 2026-09-01 ~15:52 | orchestrator: all 3 sha256 match the real files; all 4 `:lines:` ranges read back and match byte-for-byte; counts untouched | `nodes/atlas/result.md` |
| tests | verified | 2 | opus→sonnet | 40m | 2026-09-01 ~15:52 | orchestrator ran the negative control itself: 9/9 new assertions FAIL vs a self-built pre-ordinal binary, 19/19 old pass; 15/0 + 13/0 vs the real binary; jump.sh control case intact | `nodes/tests/result.md` |
| run-suites | verified | 0 | transform | 10m | — | jump 13/0, live 15/0 against the real binary (orchestrator-run) | `scratchpad/final-{jump,live}.log` |
| counts-patch | verified | 0 | transform | 2m | — | tools/atlas/sift.org 6 -> 15 assertions; grep confirms no stale "6 assertions" remains | inline |
| docs | verified | 1 | sonnet→haiku | 20m | 2026-09-01 ~16:05 | orchestrator: counts read 13/15/28 on disk; ordinal sub-table present; "left Alt" named; Home/End claim corrected + troubleshooting entry; only runbooks/sift.md touched | `nodes/docs/prompt.md` |
| use-node | verified | 1 | sonnet→haiku | 25m | 2026-09-01 ~16:12 | landed cc193 @ col 19 = orchestrator ground truth computed outside the pipeline; volatile line index not compared | `nodes/use-node/result.md` |
| use-check | verified | 0 | transform | 5m | — | structural fields match exactly; routeAfterUse -> proceed | `nodes/use-check/truth.md` |
| map-close | verified | 1 | sonnet→haiku | 25m | 2026-09-01 ~16:25 | orchestrator: t2/t3/t4 all `** DONE` with `- review:` lines; both defects in Notes; deferral sections unmodified | map file |
| G1 (commit gate) | **approved** | — | gate | — | — | user chose one commit; harnesses left untracked | `nodes/G1-payload.md`, `nodes/G1-staged.diff` |
| commit | verified | 0 | transform | 5m | — | `1a9fa21`, 5 files +555/-24, staged by explicit path; working tree clean of tracked modifications | `git log -1` |

## Note on timestamps

Times marked `~` are approximations recorded at the time of writing, not clock readings; the
only measured wall-clock checks are noted as such. Corrected at 15:56 after a timer wake showed
the log had drifted ahead of the real clock — an approximate ledger is fine, one with invented
precision is not.

## Log

- 2026-09-01 15:02 run dir materialized; graph.md §§0–11 on disk before the first dispatch.
- 2026-09-01 15:07 negative control run: exit 1, correct failure mode.
- 2026-09-01 15:16 impl-a returned; verified against on-disk artifacts; routeAfterBuild -> proceed.
- 2026-09-01 15:17 wave 2: impl-b dispatched.
- 2026-09-01 15:26 composer run for the two judgment nodes (verify-impl, tests) — prompts on
  disk; why-note at `prompts/sift-ordinal-judgment-nodes.why.md`. Negative-control mechanism
  for `tests` verified by the orchestrator first (pre-ordinal binary builds into scratch;
  `tools/target/release/sift` untouched).
- 2026-09-01 15:30 impl-b returned; verified against on-disk artifacts; routeAfterBuild -> proceed.
- 2026-09-01 15:30 PRE-EXISTING Home/End decode defect reported by impl-b, INDEPENDENTLY
  VERIFIED by the orchestrator (`cat -v` probe: tmux emits ESC[1~/ESC[4~; the CSI switch's
  default arm drops them without consuming the `~`). Routed into verify-impl's prompt as a
  scope boundary and into tests' prompt as an assertion exclusion; queued for map-close + G1.
- 2026-09-01 15:31 wave 3: verify-impl dispatched (router node; atlas and tests wait on it).
- 2026-09-01 15:46 verify-impl returned: 23 implemented, 1 out-of-scope, 2 unmet, 1 BLOCKING.
  Orchestrator independently reproduced the blocking footer-wrap regression at w=74.
  routeAfterVerify -> **repair**. IMPL_ROUND 0 -> 1.
- 2026-09-01 15:47 wave 4: impl-b re-dispatched (attempt 2/2) for the footer width guard only.
- 2026-09-01 16:04 impl-b attempt 2 returned. Orchestrator verified independently: width sweep
  8/0 with a self-built unguarded CONTROL failing 5/8 at the predicted w=75 boundary; feature
  probe 13/0 at w=100 and w=74; build 0 warnings; suites 13/0 and 6/0. Blocking 1 -> 0
  (progress metric strictly decreased). routeAfterVerify -> **proceed**.
- 2026-09-01 ~15:52 wave 5: the parallel region — atlas and tests dispatched together.
- 2026-09-01 ~16:03 tests returned. Orchestrator ran the negative control independently against
  its OWN pre-ordinal binary: 9/9 new assertions fail, 19/19 old pass. The control caught one of
  the node's own assertions being vacuous (a one-digit Backspace case passed against the
  baseline by coincidence) — it was rewritten to a two-digit buffer through three pops.
- 2026-09-01 ~16:05 run-suites + counts-patch transforms done; docs dispatched and returned.
- 2026-09-01 ~16:10 use-check ground truth computed outside the pipeline: ordinal 12 = token
  cc193 at column 19. Volatility classified (`nodes/use-check/truth.md`).
- 2026-09-01 ~16:12 wave 6: use-node dispatched (runbook only).
- 2026-09-01 ~16:15 use-node returned and PASSED against ground truth — and reported that
  ordinal 12 of 12 is the DEFAULT selection, holing the discriminating power of both its own
  check and live suite §7. Routed back to the `tests` node (owner of that file) with measured
  ground truth for ordinal 5 (col 13 on row191 — differs from the default in both column and
  line). tests re-opened as attempt 2; suite-run budget raised 8 -> 12 for the re-verification.
- 2026-09-01 ~15:55 atlas returned and verified (digests + all four line anchors checked against
  the real file). Flagged pre-existing: the node has 4 Crux excerpts vs a stated contract of
  <=3 — out of scope, routed to G1. tests still in flight.

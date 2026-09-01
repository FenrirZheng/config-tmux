# tests — returned & verified

files: [records/2026-08-27-2240-tmux-sift/assets/scripts/verify-sift-live.sh]
9 new assertions in three new sections (§6 mode mechanics ×7, §7 Enter on the TARGET pane,
§8 the 74x20 narrow-width header). The pre-existing real-binding section moved §6 → §9
deliberately: it attaches a 282x71 client to the throwaway server, which would resize the
sessions the width assertion depends on. `verify-sift-jump.sh` untouched, with a stated reason.

## Orchestrator verification — run independently, both directions

| | vs. the real binary | vs. MY pre-ordinal baseline |
|---|---|---|
| 9 new ordinal assertions | **9 pass** | **9 FAIL** |
| 19 pre-existing assertions | **19 pass** | **19 pass** (6 live + 13 jump) |

The baseline was built by the orchestrator from `git show HEAD:tools/sift/src/main.cpp`
(`grep -c goto_buf` → 0, i.e. provably pre-ordinal). Every new assertion is shown capable of
both failing and passing **per-assertion**, not merely in aggregate.
Final counts: **jump 13/0, live 15/0**.
Control case at `verify-sift-jump.sh:38` ("harness targets the throwaway server") intact.

## The oracle caught a vacuous assertion — the reason it was built

The node's first Backspace assertion (one-digit buffer, two pops) **passed against the
pre-ordinal baseline by coincidence**: with a stale trailing digit in the pattern, a plain
pattern-delete produced byte-identical strings to the ordinal semantics. Nothing but the
negative control would have caught it — the suite was green against the real binary either
way. It now drives a two-digit buffer through three Backspaces asserting three distinct
states, with the reason written into the suite as a comment so it is not "simplified" back.

## Notes routed forward

- **UNASSERTED — Home/End as movers.** Pre-existing decode gap, out of scope; the exclusion is
  recorded in a comment beside the mover assertion so it is not silently rediscovered. The
  mover assertion covers Down/C-n/PgDn/Up/C-p/PgUp instead.
- The `PgUp`/`PgDn` step is `pane_height - 3`, so §6's expected values hold only because the
  live session is 30 rows and N=12 (both clamp). Recorded in the suite.
- The N=12 fixture pattern `[abc][abc]19[0-3]` is re-measured at runtime with a fail-closed
  guard if it ever drifts off 12 — and it spans 4 lines × 3 occurrences, so an ordinal is
  provably not a line number.
- **For docs**: live is now 15/0, jump 13/0; the runbook quotes 13 / 6 and "the same 19
  assertions", all three now stale.

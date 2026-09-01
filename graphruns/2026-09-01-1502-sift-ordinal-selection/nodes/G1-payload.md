# G1 — commit gate. STAGED, NOT COMMITTED. Nothing has been `git add`ed.

Full reviewable intent on disk: `nodes/G1-staged.diff` (the diff itself, not a summary)
and `nodes/G1-status.txt`.

## Tracked changes — 5 files, +555 / -24

| file | ticket | what |
|---|---|---|
| `tools/sift/src/main.cpp` | t2 | ordinal column + ordinal mode + the footer width guard (+157/-9) |
| `tools/atlas/sift.org` | t2 (+ one t3 number) | ordinal-mode prose, 4 `:lines:` re-anchored, sha256 recomputed, assertion count 6 -> 17 |
| `…/verify-sift-live.sh` | t3 | 11 new assertions in three new sections; old §6 renumbered to §9 |
| `runbooks/sift.md` | t4 | ordinal-mode sub-table, `goto>`, left-Alt, counts 13/17/30, Home/End correction |
| `…/sift-ordinal-selection.org` | t2+t3+t4 | three Resolutions, tickets flipped to DONE, two defects into Notes |

## Commit-split separability — checked, per the repo's own rule

**A clean per-ticket split is NOT file-separable.** Two files carry more than one ticket:

- `sift-ordinal-selection.org` — 9 hunks spanning all three Resolutions.
- `tools/atlas/sift.org` — 5 hunks, four of them t2 content, one the assertion count that only
  became true once t3 landed.

A three-commit split therefore needs hunk-level staging of both files, and commit 1 would
briefly claim "17 assertions" in the atlas node before the suite that has them exists.

**Recommended: one commit.** It matches the repo's stated preference — bundle a feature's
halves unless they are truly independent — and here they are not: the suite pins the
implementation, the runbook quotes the suite's counts, and the map records all three.

    sift: add ordinal mode, pick a match by typing its number (close t2/t3/t4)

## Open items the gate should decide

1. **One commit vs three** (recommendation above).
2. **`graphruns/` — leave untracked?** Precedent says yes: the previous run
   `2026-08-31-2218-sift-cpp-to-rust` is untracked too.
3. **Three hook-archived harnesses** — `…/2026-09-01-1330-…/assets/scripts/{sweep.sh,
   feature.sh,index.org}`, written by `scratch-script-capture.sh`, not by any node's edit.
   Precedent is mixed: `probe-alt-digit-cpp.sh` in the same dir IS tracked (t1's Resolution
   links it), while the 2026-08-27 effort's equivalents were left untracked. `sweep.sh` and
   `feature.sh` are single-use probes superseded by the permanent suite assertions, so my
   recommendation is to leave them untracked — but they are cited nowhere, so nothing breaks
   either way.

## Deferrals carried forward — explicitly waived, not silently shipped

- The ordinal column's jittering width (map "Not yet specified") — ADR-0007 accepts it.
- Any further in-popup indicator beyond `goto>` (map "Not yet specified") — deliberately
  not invented.
- **`Home`/`End` have never worked in sift** — pre-existing decode gap, recorded in Notes as a
  candidate for its own ticket. The runbook now says so instead of claiming they work.
- `tools/atlas/sift.org` has 4 Crux excerpts against a `<=3` format contract — pre-existing.
- Two runbook gaps the use-node found (the non-popup launch case; timing sensitivity unstated).

## Scope of the verification claim

Every node was re-verified by the orchestrator against its on-disk artifacts. One precise
limit: the 25-item **agent** verifier ran against the pre-repair source; the footer repair was
verified by orchestrator transforms (width sweep 8/8 with a control failing 5/8, feature probe
13/13 at two widths, both suites) rather than by re-running that agent.

# use-node — returned & verified. PASSED, and it found a hole in its own check.

method: **ordinal** · keystrokes `[abc][abc]19[0-3]  M-1  2  Enter`
headerBeforeJump: `goto> 12    12 matches`
landedRaw: `1|19|cc193|row193 aa193 bb193 cc193`

## use-check — ground truth computed OUTSIDE the pipeline (nodes/use-check/truth.md)

| field | class | expected | observed | verdict |
|---|---|---|---|---|
| cursor word | structural | `cc193` | `cc193` | **match** |
| `copy_cursor_x` | structural | 19 | 19 | **match** |
| `pane_in_mode` | structural | 1 | 1 | **match** |
| absolute line index | VOLATILE | — | 195 | not compared, by design |

`routeAfterUse` → **proceed**. A fresh agent, given ONLY `runbooks/sift.md`, reached the
named match by the documented method and landed exactly where the headless seam said it
should. The runbook is sufficient for its stated job.

## The finding that matters more than the pass

The node reported, unprompted:

> match 12 was already the default-selected row before I touched ordinal mode at all

It is right, and this holes the check I designed: `refilter()` seats `sel` on the last hit
(the nearest-bottom-first bias), so **ordinal 12 of 12 IS the default selection**. An
implementation that entered the mode and ignored every digit would have landed in the same
place. The use-node still demonstrated the doc is followable — it derived `M-1` `2` from the
runbook alone — but the *landing* proves less than intended.

The same hole is in the shipped suite: live **§7 asserts ordinal 12 of N=12**. It is not
vacuous (the negative control reddens it, because on the pre-ordinal binary the `2` lands in
the pattern and the jump changes), but it would pass on a build where `push_ordinal_digit`
was a no-op.

**Routed back to the `tests` node** (the owner of that file) rather than patched by the
orchestrator, with measured ground truth for **ordinal 5** — column 13 on `row191`, which
differs from the default in *both* column and line, so it discriminates a no-op digit buffer
and a line-level ordinal at once. Volatility restated: assert the column and the line text,
never the absolute line index.

## Two documentation gaps reported (real, minor, routed to G1)

1. The runbook documents the popup case only. Launching `sift %0` interactively from a
   *different* window leaves the copy-mode state on the **target** pane, not the pane sift was
   typed into — the node's first `display-message` hit the wrong pane and came back
   `pane_in_mode=0` with blank fields. It recovered by trial.
2. The key table does not say whether bare-digit buffering has any inter-keystroke timing
   sensitivity. Not encountered as a problem; not ruled out either.

Neither blocks. Both are candidates for a follow-up runbook line rather than defects.

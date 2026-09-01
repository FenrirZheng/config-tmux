# atlas — returned & verified

files: [tools/atlas/sift.org]

- Re-anchored all four Crux excerpts: 449-459 → **465-475**, 465-473 → **481-489**,
  492-494 → **508-510**, `parse_fields<N>` unchanged at **220-231**.
- Recomputed `:SOURCES:` — `main.cpp` digest replaced; the two CMake digests re-derived and
  found unchanged.
- One new Summary paragraph folds in ordinal mode in the node's own voice: the entry key was
  free because those bytes were already decoded and discarded; `Ui::goto_buf` alone is the
  mode so no flag can drift; buffer and selection are one object so `Enter` confirms rather
  than wagers; and the footer guard exists because `draw()` is contracted to emit exactly `h`
  lines.
- `tools/atlas/index.org` left alone with a stated reason (ordinal mode is UI-loop state, not
  new tmux arithmetic, so the summary's plumbing/arithmetic split still holds).
- Assertion counts untouched, as instructed — `counts-patch` owns them.
- `atlas-stale --crux` and `atlas-links` both clean.

## Orchestrator verification (against the files, not the self-report)

- `sha256sum` on all three sources → **all three digests match** what the node records.
- **Each of the four `:lines:` ranges read back from the current `main.cpp` and compared to the
  embedded excerpt: all four match byte-for-byte.** This was the node's whole reason for
  existing, so it was checked rather than accepted.
- Line 18 still reads "13 assertions" / "6 assertions" — counts genuinely untouched.

## Flagged, out of scope, routed to G1

The node carries **4 Crux excerpts where the atlas format contract states ≤3** — pre-existing,
predates this effort. The node reported it rather than silently restructuring, which is the
right call; recorded here so it is a decision rather than an oversight.

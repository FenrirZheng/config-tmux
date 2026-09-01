# impl-a — returned & verified

buildClean: true / warnings: 0 / jump 13/0 / live 6/0
files: [tools/sift/src/main.cpp] (+22 / -1)
widthArithmetic: textw = w - numw - ordw - 4  (replaces w - numw - 3)
Ordinal rendered in kCyan (\x1b[36m) with kDefaultFg reset; line number stays kDim —
the "tell them apart at a glance" choice. ordw = digits of u.hits.size(), recomputed
every draw(), so it jitters with N exactly as ADR-0007 accepts.

## Orchestrator verification (against on-disk artifacts, not the self-report)

- `git status --porcelain` → only ` M tools/sift/src/main.cpp` (plus pre-existing untracked).
- `cmake --build --preset release --clean-first` → rc 0, **0 warnings** (independent re-run).
- `verify-sift-jump.sh` → passed 13, failed 0 (independent re-run).
- `verify-sift-live.sh` → passed 6, failed 0 (independent re-run).
- Diff read: ordinal cell emitted immediately left of the line-number cell; `refilter`,
  `u.sel`, the `> ` marker, `Enter` and the header untouched.

routeAfterBuild(...) → **proceed**.

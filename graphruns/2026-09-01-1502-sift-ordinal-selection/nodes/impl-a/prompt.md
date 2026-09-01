You are implementing the FIRST, THINNEST slice of ADR-0007 in a C++20 program. This is a
walking skeleton: it must be end-to-end working and leave both existing verification
suites green. Do NOT implement ordinal mode itself — a later node does that.

## Repo and build

Repo: `/home/fenrir/.tmux` (branch `main`, single worktree). File to edit:
`tools/sift/src/main.cpp` — and ONLY that file.

Build (never `cargo`, this is the one C++ tool in the repo):

    cd /home/fenrir/.tmux/tools/sift
    cmake --preset release && cmake --build --preset release

The compile bar is the warning set already in `CMakeLists.txt`
(`-Wall -Wextra -Wpedantic -Wconversion -Wsign-conversion -Wold-style-cast -Wshadow`).
It must stay at **zero warnings**. This change adds ordinal↔index↔cell conversions, so
`-Wconversion`/`-Wsign-conversion` are exactly the flags that will bite: use explicit
`static_cast` in the style the file already uses, never a C-style cast.

## Read first (in this order)

1. `docs/adr/0007-select-a-sift-match-by-typing-its-ordinal.org` — the settled design.
   Your slice is the first bullet of "The interaction, as settled".
2. `tools/sift/src/main.cpp` — in particular `draw()`, `render_line()`, `struct Ui`.
3. `tools/atlas/sift.org` — the subsystem node. Read it for context; do NOT edit it.

## Your slice, exactly

**The ordinal column renders always, left of the line-number column, at the width of the
current N** (N = `u.hits.size()`), and nothing else changes.

- The ordinal of a row is its 1-based position in `u.hits` — so the row at `u.hits[i]`
  shows `i + 1`. The bottom-most-selected default (`refilter` sets `u.sel = size-1`) is
  unchanged.
- Column width = the number of decimal digits in N. When the list is empty there are no
  rows to render, so the width question does not arise.
- It must share `draw()`'s width budget with the line-number column and `render_line()`'s
  cell arithmetic. Today the text field is `w - numw - 3`; after your change the ordinal
  column and its separating space must be subtracted too, and **the horizontal-scroll
  guarantee in `render_line()` (the match is always on screen, long lines scroll) must
  survive the narrower text field**. Re-derive the arithmetic; do not guess a constant.
- Render it in the established style — the file uses `std::format` with `{:>{}}` for the
  right-aligned line number and `kDim`/`kUnbold` around it. Make the ordinal column
  visually distinguishable from the line-number column rather than a second identical
  dim number; the user has to be able to tell at a glance which one they would type.
- The selected row's `> ` marker, `Enter`'s jump, `refilter`, and the header are all
  untouched.

Known open question you must NOT try to settle (ADR-0007 and the map both defer it): the
column width tracks N and therefore jitters as the pattern changes. That is the accepted
behaviour for now — implement the tracking width, do not pin it.

## Prove it

1. Build clean, zero warnings. Report the exact warning count.
2. Run BOTH suites and report their `passed N, failed M` lines verbatim:

       bash /home/fenrir/.tmux/records/2026-08-27-2240-tmux-sift/assets/scripts/verify-sift-jump.sh
       bash /home/fenrir/.tmux/records/2026-08-27-2240-tmux-sift/assets/scripts/verify-sift-live.sh

   The pre-change baseline is **jump 13 passed / 0 failed** and **live 6 passed / 0
   failed**. Your slice must not move either. If a suite reddens, that is your defect to
   fix, not a fact to report and move on from.
3. Eyeball the column: launch sift against a throwaway tmux server (copy the pattern from
   `records/2026-09-01-1330-sift-ordinal-selection/assets/scripts/probe-alt-digit-cpp.sh`
   — a `-L` socket, never `$TMUX`), type a pattern, `capture-pane` the popup, and paste
   two or three rendered rows into your result block so the column is visible as evidence
   rather than asserted.

## Hard prohibitions

- Never `git add`, `git commit`, `git checkout`, or `git stash` anything. Leave your work
  as uncommitted working-tree changes — a later gate handles committing.
- Never touch `$TMUX` or the user's real tmux server. Throwaway `-L <name>` sockets only.
- Never run `cargo` in `tools/sift`.
- Edit no file but `tools/sift/src/main.cpp`.
- Do not leave build artefacts outside `tools/target/` (the cmake preset already writes
  there); do not use a compiler invocation as a syntax check that drops a `.o`/`.gch`
  beside the source.
- ≤ 6 cmake invocations.

## Output contract

Write no report files; your deliverable is the edited source. **On failure, write NO
artifact** — revert your edit with a targeted rewrite (not `git checkout`) and report the
failure in the result block only. **Return a terminal result — do not background any
self-check and do not end your turn waiting on one.**

End with exactly this fenced block:

```result
buildClean: true|false
warnings: <int>
jumpPass: <int>
jumpFail: <int>
livePass: <int>
liveFail: <int>
files: [tools/sift/src/main.cpp]
widthArithmetic: <one line: the new text-field width expression and what it replaced>
renderedRows: |
  <2-3 captured popup rows showing the ordinal column>
notes: <anything the next node must know; "none" if nothing>
```

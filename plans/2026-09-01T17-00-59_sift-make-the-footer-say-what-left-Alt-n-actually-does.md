---
captured: 2026-09-01 17:00
session: 4fe3a1de-bb6d-4dc2-b6cc-78a34030775a
project_dir: /home/fenrir/.tmux
cwd: /home/fenrir/.tmux
transcript: /home/fenrir/.claude/projects/-home-fenrir--tmux/4fe3a1de-bb6d-4dc2-b6cc-78a34030775a.jsonl
source: ExitPlanMode (PostToolUse hook)
plan_source: /home/fenrir/.claude/plans/ui-sequential-parrot.md
---

# sift: make the footer say what `left-Alt-<n>` actually does

## Context

`sift` (`prefix /`) gained ordinal mode this week (ADR-0007, shipped in `1a9fa21`):
every result row carries a cyan 1-based ordinal, and left `Alt-<digit>` enters a
transient mode that selects a match by its number.

Having used it, the user asks that the UI say what `alt+1` does. The key *is*
already named in the footer —

    ↑↓ select  left-Alt-digit goto  Enter jump  Esc cancel  C-w word  C-u clear

— so the gap is not that the key is undocumented on screen. It is that **`goto`
names the action without naming its object**: nothing on the line says the digit
selects the numbered match, so "goto *what*" is left to inference.

This request is also the decision condition for an item the map deliberately
deferred (*Not yet specified*, `sift-ordinal-selection.org:104-106`):

> Whether ordinal mode wants any indicator beyond the `goto>` prompt once it is in
> daily use — the popup header has room, but inventing one before the feature has
> been lived with is guessing.

The feature has now been lived with, so the item is decidable. The user chose the
minimum: **reword the footer**. A header hint and a per-row marker were both offered
and declined.

**Outcome**: a reader of the popup can tell, without leaving it, that left-Alt plus a
digit goes to the match carrying that number.

**Honest ceiling, to be recorded in t5 rather than glossed**: a static dim line is
skipped by the eye regardless of its wording, which was half the original complaint;
this change does not address that half. The deferred question ("*any* indicator
beyond `goto>`") is therefore being closed by a narrower answer than it asked for.

## The change

```diff
-        "↑↓ select  left-Alt-digit goto  Enter jump  Esc cancel  C-w word  C-u clear";
+        "↑↓ select  left-Alt-<n> goto match n  Enter jump  Esc cancel  C-w word  C-u clear";
```

`tools/sift/src/main.cpp:889-890`. Footer goes **75 → 81 cells**.

Why `match n` and not `#n`: `match` is the only candidate token with an on-screen
referent chain — the header's `12 matches` (`main.cpp:833-835`), the cyan ordinal
column, and the glossary term *Match ordinal*. `#` in this repo is tmux format syntax
(`#{pane_id}`, `#{copy_cursor_x}`), and the rows render bare numbers, so `#n` would
add an inference instead of removing one.

Two wording traps:

- **lowercase `n`, never `N`** — capital *N* is ADR-0007's and the glossary's symbol
  for the *total*, so `goto match N` would say "go to the last one".
- **keep the angle brackets** — `left-Alt-n` reads as Alt plus the letter `n`, a real
  key nobody should press. `<n>` is what marks it a placeholder.
- Do not call this a "hint" in any prose: `CONTEXT.org:14` lists `hint` under **Avoid**
  for *Match ordinal* (reserved for the relabel scheme ADR-0007 rejected). Write
  "footer item". Likewise avoid "goto mode" — Avoid list for *Ordinal mode*.

## Constraints

1. **`draw()` emits exactly `h` lines** — header 1 + `list_rows` (`h-2`) + footer 1
   (`main.cpp:815-825`, `:892-902`). A footer one cell over the pane width wraps, the
   pane scrolls, and the header — carrying the `goto>` prompt *and* the match count —
   leaves the screen. That regression was caught in review this week; `utf8_fit`
   (`main.cpp:310-325`) is the guard.
2. **Do not tune the footer toward 74 cells.** A shorter form (`left-Alt-n goto #n`,
   74) would make the footer fit exactly at the width §8 tests — and §8 exists
   *because* 74 is below the footer width, so the guard must act. At 74 the assertion
   would pass on a guarded *and* an unguarded binary: it keeps its wording and loses
   its teeth. 81 keeps §8 fully sharp.
3. **House style**: items are `<key> <lowercase verb>` joined by exactly two spaces;
   whole line `kDim`, no per-key colour (`kCyan` is reserved for the ordinal column,
   `main.cpp:680-682`).

## Files to change

| file | change |
|---|---|
| `tools/sift/src/main.cpp:889-890` | the `kFooter` literal — the only functional change |
| `records/2026-08-27-2240-tmux-sift/assets/scripts/verify-sift-live.sh` | two assertions folded into §8 (below); header comment `:11-15` "eleven" → "thirteen"; append a clause to `:215` |
| `tools/atlas/sift.org` | `:83` recompute the `main.cpp` sha256; `:18` assertion count 17 → 19 |
| `runbooks/sift.md` | `:119` 17 → 19; `:129` 30 → 32; `:34` quote the shipped footer text so doc and binary match |
| `records/2026-09-01-1330-sift-ordinal-selection/sift-ordinal-selection.org` | new ticket t5; strike the *Not yet specified* bullet at `:104-106`; add its decision to *Decisions so far*; one pointer line in t4 |

Checked and **clear** — state this so nobody re-checks: ADR-0007 does not quote the
footer verbatim (`:91` says only that it must say "left Alt"), so the immutable ADR is
untouched. `tools/atlas/sift.org:16` describes the guard width-agnostically.
`verify-sift-jump.sh` never mentions the footer. The runbook's «**The `goto>` prompt is
the only indicator that the mode is live**» (`:50-51`) stays true — the footer is not a
liveness indicator; do not "update" it. Remaining verbatim quotes live under
`graphruns/` and `plans/`, which are run logs and already dated.

## Verification

**Reject the obvious assertion.** "The hint survives `utf8_fit`'s cut" is near-vacuous:
`utf8_fit` cuts from the *right* and the new item sits at cells 12–36, so it is true at
any width ≥ 36 and can never be falsified by a future footer addition — additions land
after it. What growth threatens is the tail, `C-u clear`.

**Assert the whole footer line at exactly its own width.** One equality carries both
the item's presence and the no-truncation property, and at exact fit it is maximally
sensitive: one added cell anywhere on the line reddens it.

Fold into §8 (`verify-sift-live.sh:214-241`) rather than adding a §10 — avoids
renumbering §9 and keeps width concerns together. Retitle §8 to cover both. Add a
second throwaway session:

- `new-session -d -s fitw -x 81 -y 20`, then the same `new-window` / `M-1` shape as
  `:226-236`.
- **Fail closed on `#{pane_width}x#{pane_height}` = `81x20`**, exactly as `:230` does —
  a pane that is not the footer's width cannot test the budget.
- (a) `sed -n 1p` still begins with `goto> ` — the `h`-line contract, and the
  diagnostic that separates "footer wrapped" from "footer merely wrong".
- (b) `sed -n 20p` **equals** the exact 81-char footer string. Line 20, not `tail -1`,
  not a grep — the rigour `:221` already argues for.
- Comment must state that `-x 81` and the expected string are two encodings of one
  measurement and move together, and that the string is transcribed from
  `main.cpp:889-890`.

The shipped 266x62 geometry needs no second assertion: `utf8_fit` only ever cuts, so a
line that renders whole at its own width renders whole at every larger width.

**Negative control is free, and it dictates the order of work.** `1a9fa21` is the
current working tree, so:

1. Add the two assertions **first**, before touching `main.cpp`. Run the live suite
   against the already-built `tools/target/release/sift` → expect **17 pass / 2 fail**
   ((b) fails on the string, (a) passes).
2. Change the literal, `cmake --preset release && cmake --build --preset release`,
   rerun → expect **19 / 0**, and `verify-sift-jump.sh` still 13/0.

No scratch build and no risk of overwriting the binary `prefix /` runs.

**§8's existing 74x20 assertion is unchanged and keeps its teeth** — 74 < 75 and
74 < 81, so the footer is cut in both worlds. Only its comment changes: `:215` says
"the footer had grown to 75 cells", which is *historically accurate* about the
2026-09-01 defect. Do not rewrite the number — append "(81 cells since t5; the guard's
behaviour at this width is unchanged)", or a future reader reconciling 75 against
`main.cpp` will "fix" it and erase the regression's history.

**One thing to measure, not assume**: that a line exactly filling the pane width does
not itself wrap at 81. Evidence exists at 75 (t2's unguarded control passed there with
a 75-cell footer) but was not taken at 81. Confirm during implementation; if tmux does
wrap, move the canary to 82 and say why in the comment.

## Record-keeping

The map's t2 and t4 Resolutions both quote the old footer. **They are not the same
case and do not get the same treatment.**

- **t2 (`:233`) — leave untouched.** That quote is the *input to a measurement*: "is 75
  display cells, up from 54, and `draw()` emitted it with no width guard at all… at any
  sift width ≤ 74 the footer wrapped". That statement is true forever. The string moved
  on; the measurement did not. Stamping "superseded" on it would imply the finding was
  displaced, which is false. Same reasoning covers its recorded 5/8 unguarded-control
  result at the w=75 boundary — dated, correct, and left alone.
- **t4 (`:382`) — one pointer line.** t4 is not a measurement but *"the decision this
  ticket asks to record"* about wording, and a decision record with no forward pointer
  is the one case where a reader can act on stale information. Add, after that
  paragraph: "Superseded as to wording by [[#t5]] (2026-09-01); the 'left Alt'
  rationale below is unchanged." The second clause matters — t5 revises the surface,
  not the reason.
- **Rewriting either is out.** It falsifies a record of what was measured when, and
  t2's 75 is quoted by `verify-sift-live.sh:215`; desynchronising a record from a
  comment that cites it is how the history gets lost.

**t5** records: the decision condition (settled by use, not invention), the wording and
why `match n` beat `#n`, the 75 → 81 move and its effect on the guard boundary, the new
assertion counts, and the honest ceiling from the Context section. **ADR-0007 is not
touched** — accepted, implemented, immutable, and this is a wording change inside a
decision it already made.

## Verify end-to-end

```bash
cd ~/.tmux/tools/sift && cmake --preset release && cmake --build --preset release   # 0 warnings
bash ~/.tmux/records/2026-08-27-2240-tmux-sift/assets/scripts/verify-sift-jump.sh   # 13/0
bash ~/.tmux/records/2026-08-27-2240-tmux-sift/assets/scripts/verify-sift-live.sh   # 19/0
```

Then look at it: `prefix /`, type a pattern that matches, and read the footer.
Then `prefix /` in a pane narrowed below 81 columns and confirm the header is still
on line 1.

Commit as one change (code + suite + docs + map are not independent halves), matching
the repo's `close tN:` subject style.

You are closing three tickets on a wayfinder map by writing their Resolutions from measured
evidence. **Default stance: every number and claim you write must come from the evidence block
below — if it is not there, you do not know it.** These Resolutions are the durable record;
a plausible-sounding unmeasured detail in one is worse than an omission.

## RUNNER FILLS (if any path is missing, STOP and say so — do not guess)

- `{{MAP}}` = `/home/fenrir/.tmux/records/2026-09-01-1330-sift-ordinal-selection/sift-ordinal-selection.org`
  — the file you edit, and the ONLY one
- Read the **closed ticket t1** in that file first. Its Resolution is the house style: what was
  measured, how, what it means for the ADR, and a closing `- review:` line. Match it.

## Your job

For **t2**, **t3** and **t4**: change `** TODO` to `** DONE`, fill `:ASSIGNEE:` with
`fenrir@graphrun-1502`, and write a `*** Resolution` section under each, from the evidence
below. Then update the map's own Notes/sections where the evidence says they are now stale.

Do **not** touch the Destination, "Out of scope", or the two "Not yet specified" items — those
are deliberate deferrals and they stay exactly as they are.

## EVIDENCE — everything below was measured this run

### t2 — Build ordinal mode into the C++ sift

Built in `tools/sift/src/main.cpp` in two slices: first the always-rendered ordinal column,
then the mode itself. Final diff **+157 / −9**, one file.

- Row layout is now `"> "` (2) + ordinal (`ordw`) + space + line number (`numw`) + space + text,
  so the text field is `textw = w - numw - ordw - 4` (was `w - numw - 3`). The ordinal renders
  cyan, the line number stays dim, so the two are distinguishable at a glance. `ordw` is
  recomputed every `draw()` from `u.hits.size()`, so the column jitters with *N* exactly as
  ADR-0007 records and accepts — nothing pins it.
- Entry is the `c1` branch of `read_key()`, as t1 predicted: `ESC` + `'0'..'9'` now returns a
  new `Key::AltDigit` carrying the digit, where it previously fell into `Key::None`. The 40 ms
  `ESC_SEQ_MS` window is untouched.
- `Ui::goto_buf` is the entire mode state — non-empty **is** the mode, so there is no separate
  flag that could drift out of sync. Helpers: `in_ordinal_mode`, `ordinal_of`,
  `push_ordinal_digit`, `sync_ordinal`. One bounds check in `push_ordinal_digit` implements all
  four refusals at once: out-of-range, `Alt-0`, empty list, invalid pattern.
- `draw()` selects the live prompt (`goto> ` / `regex> `), recomputes `plen` from it, and parks
  the cursor at the end of whatever is being typed. Measured `cursor_x`: 14 for
  `regex> hitline`, 7 for `goto> 1`, 8 for `goto> 12`.
- `jump()`, `run_rows()`, `refilter()` and `render_line()` untouched. `sift rows` gains nothing,
  as ADR-0007 says it should not.
- Build: `cmake --preset release`, **zero warnings** under
  `-Wall -Wextra -Wpedantic -Wconversion -Wsign-conversion -Wold-style-cast -Wshadow`.
- Suites unmoved by the implementation: **jump 13/0, live 6/0** before and after.
- `tools/atlas/sift.org` updated in the same change, as the map's Notes require: all four Crux
  `:lines:` excerpts re-anchored (449-459 → 465-475, 465-473 → 481-489, 492-494 → 508-510,
  `parse_fields<N>` unchanged at 220-231) and verified byte-for-byte against the file; the
  `main.cpp` sha256 in `:SOURCES:` recomputed.

**Three decisions taken beyond ADR-0007's literal text — record these explicitly, they are the
part a future reader will not be able to derive from the ADR:**

1. **The invariant extended to every mover.** ADR-0007 names only `↑↓` as rewriting the buffer,
   but its stated invariant — "`goto>` always names where the cursor actually is" — does not
   hold unless every mover does, so `sync_ordinal()` is called from `Up`, `Down`, `Home`, `End`
   and the `PgUp`/`PgDn` block.
2. **`C-w` / `C-u` clear the buffer before touching the pattern.** The ADR does not name them.
   Chosen over ignoring them so the load-bearing property "no exit from the mode happens after
   the pattern changes" stays true — both call `refilter()`.
3. **`Alt-<digit>` pressed while already in the mode extends the buffer**, exactly like a bare
   digit, rather than restarting it.

**A blocking regression was found by an independent verifier and fixed** — record it, it is the
most useful thing in this ticket:

The new footer (`↑↓ select  left-Alt-digit goto  Enter jump  Esc cancel  C-w word  C-u clear`)
is 75 display cells, up from 54, and `draw()` emitted it with **no width guard at all**.
`draw()` is contracted to emit exactly `h` lines — header + `h-2` rows + footer — so at any
sift width ≤ 74 the footer wrapped, the pane scrolled, and the **header went off the top of the
screen, taking the `goto>` prompt and the match count with it**. The user would be in a mode
with no on-screen evidence of it. `claude.conf:207` sizes the popup at `-w 95%`, so any tmux
client narrower than ~81 columns was affected; the user's own 282-column client was not.
It contradicted ADR-0007's settled "the mode is never invisible", so it blocked.
Fixed by a new `utf8_fit(std::string_view, int budget)` — the forward inverse of the existing
`utf8_cells`, built from the same decoder, cutting on a character boundary; `kDim`/`kReset` are
zero-width and are neither charged against the budget nor cut. The footer *text* is unchanged,
so ADR-0007's "left Alt" wording and every doc quoting the footer still match the binary.
Verified by a width sweep at w = 20, 30, 40, 60, 74, 75, 100, 265 — **8/8 pass**; and the sweep
was shown to have teeth by running it against an unguarded control binary differing only in the
reverted guard line: **5/8 FAIL**, at exactly the predicted w=75 boundary.
Worth saying plainly: the verifier's behavioural lens passed **25/25 at 100 columns** while the
feature was invisible at 74. Only a lens aimed at "can the user actually see the number" caught
it.

The independent verifier walked all 25 of ADR-0007's settled decisions with pasted evidence
from the running binary: 23 implemented, 1 out-of-scope (below), 1 blocking (the footer, now
fixed), plus the docs gap that t4 then closed.

### t3 — Assert ordinal mode in the verification suites

`records/2026-08-27-2240-tmux-sift/assets/scripts/verify-sift-live.sh` gains **11 assertions**
in three new sections: §6 mode mechanics (7), §7 Enter on the TARGET pane (3), §8 the narrow
pane (1). The pre-existing real-binding section moved §6 → §9 deliberately — it attaches a
282x71 client to the throwaway server, which would resize the sessions §8 depends on.

**New counts: jump 13/0, live 17/0** — replacing the 13/0 and 6/0 that the runbook and the
atlas node quoted (both now updated to 13 / 17, and the sanitizer section's "19 assertions" to
30).

`verify-sift-jump.sh` gains nothing, with a stated reason: ADR-0007 leaves `sift rows`
untouched, so the k-th-row-is-ordinal-k mapping is a property of the already-pinned row
ordering rather than of ordinal mode, and an assertion over it would pass identically against a
pre-ordinal binary — vacuous by construction. Its control case ("harness targets the throwaway
server") is intact.

**The acceptance criterion was a negative control, not a green bar.** A pre-ordinal binary was
built from `git show HEAD:tools/sift/src/main.cpp` (confirmed pre-ordinal by `grep -c goto_buf`
→ 0) and both suites run against it: **11/11 new assertions FAIL, 19/19 pre-existing pass**.
Against the real binary: 17/0 and 13/0. Every new assertion is therefore shown capable of both
failing and passing, per-assertion.

**Two assertions that looked fine and were not — both caught only by the control. Record both;
they are the same class of defect and the reason the control exists:**

1. The first `Backspace` assertion (one-digit buffer, two pops) **passed against the pre-ordinal
   baseline by coincidence**: with a stale trailing digit in the pattern, a plain pattern-delete
   produced byte-identical strings to the ordinal semantics. Rewritten to drive a two-digit
   buffer through three pops asserting three distinct states.
2. **The default-selection trap.** `refilter()` seats the selection on the last hit, so
   ordinal 12 of *N*=12 **is** the default — a build whose `push_ordinal_digit` was a no-op
   would still land there. Found by the use-node (below), which noticed the match was already
   selected before it pressed anything. §7's discriminator is now ordinal **5**: column 13 on
   `row191`, differing from the default on *both* the column and the line axis. The control
   shows the trap concretely — the pre-ordinal binary lands on `19|1|1|row193 …`, i.e. exactly
   the default. Note for the future: any edit changing the fixture's *N* or the hit order must
   re-derive a non-default ordinal rather than reuse 12.

Only structural fields are compared — the column and the matched line's text. The absolute
scrollback index (193 vs 195 across servers) is volatile and never asserted for equality.
The N=12 fixture pattern `[abc][abc]19[0-3]` is re-measured at runtime with a fail-closed guard
if it drifts, and it spans 4 lines × 3 occurrences, so an ordinal is provably not a line number.

**UNASSERTED, recorded rather than dropped**: Home/End as movers — see the defect below. The
mover assertion covers `Down`/`C-n`/`PgDn`/`Up`/`C-p`/`PgUp` instead, with the exclusion written
into the suite as a comment beside it.

### t4 — Document ordinal mode where the rest of sift is documented

`runbooks/sift.md`: the key table gains a `left Alt-<digit>` row pointing at a new **Ordinal
mode** sub-table covering entry and its three refusals, digit extension and the silent
out-of-range refusal, the movers rewriting the buffer, `Backspace` popping and the last pop
leaving the mode, `C-w`/`C-u` clearing the buffer first, `Alt-<digit>` extending while already
in the mode, the non-digit fallthrough, `Esc` leaving the mode, and `Enter` unchanged. The
`goto>` prompt is explained as the sole in-popup indicator that the mode is live, with the match
count still on the right. Counts updated to 13 / 17 / 30.

**The decision t4 asks to record.** The footer says `left-Alt-digit goto` — "left" specifically,
because `~/.config/keyd/default.conf:40` gives `rightalt` to the fcitx5 IME toggle at the kernel
level, so a footer saying only "Alt" would mislead a reader who tries the right-hand key. The
`goto>` prompt is the only in-popup indicator; **no further indicator was invented**, because
the map defers that question on purpose. That deferral stays in "Not yet specified" untouched.

**Validated by use, not by inspection.** A fresh agent was given *only* `runbooks/sift.md` — no
source, no ADR, no suites — and told to reach the 12th of 12 matches. It used `M-1` `2` `Enter`,
derived from the runbook alone, and landed on `cc193` at column 19, matching ground truth
computed independently beforehand from `sift rows`. It also reported two real gaps, which are
**not** closed and should be recorded as such:
- the runbook documents the popup case only, so launching `sift %0` interactively from a
  different window leaves it unstated which pane holds the resulting copy-mode state;
- the key table does not say whether bare-digit buffering has any inter-keystroke timing
  sensitivity.

### Two pre-existing defects discovered this run — add them to the map's Notes (or "Out of scope"), NOT as ticket work

1. **`Home` / `End` have never worked in sift.** tmux emits `ESC [ 1 ~` / `ESC [ 4 ~`; sift's CSI
   decoder handles only `ESC [ H` / `ESC [ F`, so the `default:` arm returns `Key::None`
   **without consuming the trailing `~`**, which then lands in the pattern as printable text.
   Measured directly this run with `cat -v` on a throwaway server, and again live (`send-keys
   End` produced `regex> hitline~`). Unchanged by this effort — not introduced, not fixed. It is
   why `sync_ordinal()` on `Key::Home`/`Key::End` is dead in practice, and why t3 could not
   assert those two movers. The runbook's key table used to claim they "jump to the first / last
   match"; t4 corrected that to "**do not work** — see Troubleshoot" and added a troubleshooting
   entry. **A candidate for its own ticket.**
2. `tools/atlas/sift.org` carries **4 Crux excerpts where the atlas format contract states ≤3** —
   pre-existing, predates this effort, flagged rather than silently restructured.

## Style

Match t1's Resolution: bold lead-ins for the parts that matter, measured numbers inline, links
in the map's existing `[[file:...]]` / `[[#id]]` style, and close each Resolution with a
`- review:` line naming how it was checked. For these three that is: *independent verifier
(25 decisions, pasted captures) + orchestrator re-verification of every node against its
on-disk artifacts* for t2; *negative control against a pre-ordinal binary, 11/11 fail and 19/19
pass* for t3; *use-node given the runbook alone, landed on independently-computed ground truth*
for t4.

## NEVER

- Never edit any file but `{{MAP}}` — every other file is another node's work, already verified.
- Never `git add`, `git commit`, `git checkout` or `git stash` — a later gate handles committing.
- **Never write a number or claim not in the evidence above** — these Resolutions are what a
  future reader will trust instead of re-measuring, so an invented detail propagates silently.
- Never mark a ticket DONE whose evidence above shows unfinished work, and never quietly drop
  one of the recorded gaps or defects — they are routed items, not decoration.
- Never touch the Destination, "Out of scope", or the two "Not yet specified" items.

## Escape hatch

If the evidence above contradicts something already written in the map, do not silently pick a
side: write the Resolution from the evidence and note the contradiction in your result block.

## Output contract

**On failure, write NO artifact** — report in the result block only. **Return a terminal
result — do not background any self-check.**

End with exactly this fenced block:

```result
files: [records/2026-09-01-1330-sift-ordinal-selection/sift-ordinal-selection.org]
ticketsClosed: [t2, t3, t4]
notesAdded: <what you added to Notes / Out of scope, one line each>
deferralsUntouched: true|false
contradictions: <any, or "none">
notes: <anything the commit gate must know; "none" if nothing>
```

## BEFORE YOU ANSWER, re-check

Stop when t2/t3/t4 are `** DONE` with Resolutions written only from the evidence above, the two
pre-existing defects are recorded, and the Destination / Out of scope / Not yet specified
sections are untouched. Output is the `result` skeleton and nothing else after it.

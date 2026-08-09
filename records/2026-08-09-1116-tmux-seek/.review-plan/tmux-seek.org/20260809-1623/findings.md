# Delta review — tmux-seek map, 2026-08-09 16:23

Run by `/review-plan … delta` on [`tmux-seek.org`](../../../tmux-seek.org).
Baseline: [`20260809-1338`](../20260809-1338/findings.md) (the failed destination-reached gate).

**Diff reviewed** — three pure-addition hunks, no removals: the Notes paragraph announcing
the failed gate, seven `Decisions so far` lines, and tickets t5–t11 (~428 lines) claiming
to close the fifteen blockers. Tickets t1–t4 are byte-identical to the baseline.

**Coverage** — 6 reviewers dispatched: Architecture & Design → agy/Gemini
(independent-model read); Context & Alignment, Resilience & Edge Cases, Operability &
Observability, Security & Compliance, Execution & Milestones → read-only Claude subagents.
0 dimensions skipped. 0 reviewers failed. Security was re-dispatched despite the delta
rule's default because the baseline's own skip audit recorded that skipping it was wrong.

**agy slot** — deliberately moved off the default priority order (Security first) to
Architecture & Design: for a non-production CLI spec the default ordering is weak, and
the delta's central risk is cross-ticket consistency in a text-only artifact — exactly
what a text-only reader is good at. Security went to a subagent so it could grep tmuxlib.

**Verdict — the delta does not close the gate.** Nine blockers survive verification. The
most consequential: two of the fifteen original blockers are recorded as closed by
claims their own source measurements refute.

---

## Triage table

| # | Dimension | discuss / skip | reason |
|---|---|---|---|
| 1 | Context & Alignment | discuss | had blockers; `Not yet specified` still empty while t8/t9 carry unverified items |
| 2 | Architecture & Design | discuss | had blockers; t5–t11 supersede one another across seven tickets |
| 3 | Resilience & Edge Cases | discuss | had blockers; t6's new failure contract, t9's new extraction logic |
| 4 | Operability & Observability | discuss | had blockers; t7's message table and t10's rollback recipe are both new |
| 5 | Security & Compliance | discuss | baseline skip audited as wrong; t7 makes a load-bearing injection claim |
| 6 | Execution & Milestones | discuss | had blockers; t11 exports the execution checklist to a non-existent artifact |

---

## Fix these first

### 1. The map declares itself the specification while contradicting itself in five places

t11 decides "the records are the specification" and points ARCHITECTURE.md's `plan` cell
at this directory. The map is therefore the document an implementer builds from — and it
carries five live contradictions, none of them marked:

- **t3's per-key table (line 241) is fully stale.** It still lists the `v` key, `seek
  back`, `@seek_origin` and `copy_cursor_word` — every one deleted by t5 or t9. It is the
  only per-key contract table in the map. *(Context & Alignment; Execution; agy)*
- **Five keys vs four.** t3/t4 say "the five output keys" (lines 277, 297); t6/t8 say
  "all four keys" (441, 563). t4 also sizes cheat.txt at "four lines becoming seven" for
  five keys. *(verified by grep; Execution; agy)*
- **"All three fail-closed cases" (t3, line 253) vs one** (t7, line 491). *(Execution)*
- **The grain rule exists only as a mental composite** of t2 (hybrid), t5 (deliver
  `pane_search_string` verbatim) and t9 (trim first, extract own token). Each supersedes
  the last; all three are stated as current. *(agy/Gemini — independent-model read)*
- **The literal `prefix Space` binding is never written down.** The map mentions
  `command-prompt` exactly once (line 579), inside a parenthetical about a probe rig that
  hung. `%%` vs `%%%` escaping is undecided, and the prompt's `C-y` can insert the buffer
  seek just filled. *(Security)*

The Notes paragraph's supersession rule keys only off the destination review, which never
touched any of these — so nothing in the document tells a reader t3's table is dead.

### 2. Two "closed" blockers are closed by claims their own measurements refute

- **Repeat grabs (baseline blocker 3, defect 4).** t5 decision 4 says "Single-use keys —
  gone. `search_present` persists across repeated grabs, **and delivery now exits
  copy-mode anyway**." Those clauses are incompatible: the 1443 grill snapshot the ticket
  cites records at line 9 `left-and-re-entered copy-mode 0`. Exiting copy-mode resets the
  signal, so the second grab is not a repeat grab — and with the pane out of copy-mode,
  pressing `w` types a literal `w` into the shell, vim, or the Claude composer.
  *(Resilience; confirmed against the snapshot text)*
- **`run-shell` timing (baseline blocker 7).** t8 item 3 declares it dissolved because
  "seek no longer issues any `send-keys` into the pane during operation". But ADR-0003
  line 51 specifies a "race-free trailing `cancel` on success" — so an exit *is* sent, and
  the map never says by what mechanism. The race t8 declares gone is the race a
  late-arriving `cancel` creates against a copy-mode session the user has re-entered.
  *(Resilience; confirmed against ADR-0003)*

### 3. The rollback does not roll back

t10's recipe is `git revert` → `prefix I` → re-source. `source-file` is **additive**, and
`tmux list-keys -T copy-mode` binds none of `w`, `W`, `l`, `L` by default — verified on
this machine — so there is nothing to overwrite them. After a full rollback the four seek
keys stay bound in the live server and keep invoking a binary the revert may have
orphaned. The recipe needs an explicit `unbind -T copy-mode w W l L` (plus the guard
stubs) or a `kill-server`. *(Operability)*

---

## Remaining blockers

4. **`set-buffer` is specified without `--`.** A grabbed token starting with `-`
   (`--release`, `-rf`) is parsed as a flag. `tmux.conf:57` and `:68` — the thumbs commands
   seek replaces — both already carry `set-buffer -- {}`; the map drops it. *(Security;
   Resilience; verified in tmux.conf)*

5. **On `W`/`L` a wl-copy failure is silent by decision**, so the user sees `→ CLAUDE` and
   later pastes a stale clipboard. That is verbatim the regression `tmux.conf:63-66`
   records having already been fixed once. t6's rationale — "any failure message would be
   overwritten by `→ CLAUDE` anyway" — is self-imposed: seek chooses the call order and
   can message after to-claude returns. *(Operability blocker; Resilience should-fix —
   merged at the higher tag because the premise is refutable)*

6. **CJK and wrapped-line extraction is an open design decision, not a verification
   task.** t9 carries it as "verify before shipping". `copy_cursor_x` is a cell column and
   `copy_cursor_line` a screen line: wide characters break the column→char mapping, and a
   token wrapped across two screen lines is *unreachable* from `copy_cursor_line` at all —
   it needs `capture-pane -J`. This user writes Chinese daily. Also unstated: char-boundary
   indexing, without which a mismatch panics seek into a non-zero exit and a tmux error
   popup. *(Resilience; Execution)*

7. **The cutover's safety gate references two artifacts that do not exist.** t10 says
   big-bang is safe "because the execution checklist places the cutover after seek passes
   the runbook Verify" — but t11 exports the checklist to capture-tasks handoff material
   outside the map, and `runbooks/seek.md` is unwritten, so nothing states what Verify
   asserts. *(Execution; Operability; Context)*

8. **The whitespace branch of the grain rule silently collapses line grain into word
   grain.** t5's rule is unscoped by key: with whitespace in the query, `l`/`L` deliver
   `pane_search_string` instead of the line. *(Resilience — scope the branch to `w`/`W`)*

9. **`Not yet specified` is still empty** while at least four items remain live in the
   snapshots and inside Resolutions: t8's `[未驗證]` `-p` expansion, t9's cursor-mapping
   item, the 1545 snapshot's line-grain trimming ("an assumption rather than decided"),
   and the 1443 multi-token-after-cursor-move edge. Baseline blocker 1 has partially
   recurred — the items are now *named*, but in the Resolutions rather than the section
   built to hold them, so the map still reads as complete. *(Context; Execution)*

---

## Should-fix

- **`tmuxlib::message()` (lib.rs:356) omits `-l`** — the house helper every sibling tool
  uses. t7's entire injection defense is "every `display-message` seek issues carries
  `-l`", with no enforcement point; the obvious reuse silently re-opens baseline blocker 6.
  Name the mechanism (a `message_literal()` variant, or a direct `t::tmux([...])` call).
  *(Security; Resilience — one edit away from re-opening a closed blocker)*
- **ARCHITECTURE.md:121's `sanitize_format` mandate is left unamended** while seek is
  added to its tables as a visible, unrecorded exception. *(Security)*
- **How the text reaches `to-claude` — stdin or argv — is never stated.** to-claude reads
  stdin only and rejects unknown argv words (`parse_mode`, main.rs:73). *(Security)*
- **Spawn failure of to-claude is swallowed along with its exit code**: with to-claude
  unbuilt, `W`/`L` deliver nothing and say nothing. The load-time guard covers only the
  seek binary. Distinguish "could not execute" from "ran and failed". *(Resilience)*
- **No `run-shell` vs `run-shell -b` decision and no wl-copy wait bound** — a hung wl-copy
  inside a blocking `run-shell` stalls tmux's command queue. *(Resilience)*
- **The fail-closed message's scope is unstated**: t7 lists `seek: nothing under the
  cursor` unscoped but also "silence on `W`/`L`", so an empty grab on `W` may be wholly
  silent — baseline blocker 14's shape returning on a different key. *(Operability)*
- **The guard stub tells the user to build but not to reload**, though the guard is
  load-time. `runbooks/to-emacs.md`'s Troubleshooting row for the identical failure names
  both actions. *(Operability)*
- **t10's restore recipe oversells offline capability.** `tmux-thumbs-install.sh` is
  interactive (split, keypress, Compile/Download menu) and both branches need network, so
  offline rollback is impossible, not merely "no re-clone". The `$HOME` gitlink revert
  step is also missing. *(Resilience; Operability)*
- **The atomic cutover commit's contents omit the doc artifacts** (runbook, README index
  line, both ARCHITECTURE rows), weakening the "exactly one revert" claim. *(Execution;
  Operability)*
- **A Chinese query has no whitespace anywhere**, so token extraction grabs the whole CJK
  run rather than the match. The hybrid grain assumes whitespace-delimited words.
  *(Resilience — author's call, he knows his own search habits)*
- **t8 defers the prompt-label branch choice to implementation** while the Destination
  promises no remaining design decisions. t8 rebuts this ("both shapes are one line"), and
  the rebuttal mostly holds — but the verification ships *inside* the atomic cutover
  commit, so it needs a gate. *(agy/Gemini rated this `blocker`; downgraded after reading
  t8's rebuttal — see Triage challenges below)*
- **The three pure functions are never named in the map** (only in the 1611 snapshot), and
  the "worked edge table as fixture" covers only the path classifier — the grain splitter
  and token extractor get no fixture. *(Execution)*
- **"Every path exits 0" is the only cross-cutting acceptance criterion**, and it is
  satisfied by a binary that does nothing. Pair each with a positive observable.
  *(Execution)*
- **The alt-screen warning suffix is referenced but never spelled out** in t7's message
  table. *(agy/Gemini — independent-model read)*
- **Truncation protects the payload and lets the status line clip the diagnostic** —
  `(wl-copy failed)` and the alt-screen warning ride at the tail. Lead with the
  diagnostic. *(Operability)*
- **The `$HOME` rider has no owner or gate** yet t11 claims it makes to-emacs.md's
  bootstrap claim true (contradiction verified live at `runbooks/to-emacs.md:28`). State
  what happens if it never lands. *(Context)*
- **Out of scope holds one entry**; regex/fuzzy match, pointer movement, headless use and
  `plans/seek.md` are non-goals only inside snapshots. *(Context)*
- **The deletion half of the driver is still unstated.** t11's new driver ("thumbs cannot
  jump to arbitrary text") justifies *building* seek, not *deleting* thumbs; the deletion
  rationale lives only in 1558's rejected alternatives. *(Context)*
- **Troubleshooting still maps no message to a cause**, and `seek: nothing under the
  cursor` covers three distinct causes with one string. *(Operability)*
- **Re-pressing an output key in grabber mode is indistinguishable from live-search mode**
  — `→ CLIP` is identical in both, so a no-match search reports success on whatever the
  entry cursor sat on. The alt-screen suffix already keys on `search_present`, making the
  tell inconsistent rather than absent. *(Operability)*
- **`W`/`L` receipts name only the target pane**, so a stale post-Escape grab reaches
  Claude invisibly — against ARCHITECTURE.md's convention that a mis-send be visible.
  *(Resilience)*

## Nits

- README's `runbooks/seek.md` index line and cheat.txt:33's wording update were both
  raised in t11's *Question* and dropped from its *Resolution*. *(Operability)*
- `tools/Cargo.toml`'s workspace `members` edit appears in no artifact list. *(Execution)*
- The map never names the reload command; t10 says "re-source", t11 defers it to the
  unwritten runbook, and the 1558 snapshot already has the literal command. *(Operability)*
- The rollback recipe stops at the inner repo; Notes make blessing the `$HOME` gitlink
  mandatory. *(Operability; Resilience)*
- The rollback's final step compiles thumbs from source with no progress signal and no
  stated time bound — an operator mid-rollback reads the pause as a second failure.
  *(Operability)*
- seek should check `#{pane_in_mode}` before claiming an empty cell: the async grab can
  read `copy_cursor_*` after the pane left copy-mode, and empty formats are
  indistinguishable from a genuinely empty cursor cell. *(Resilience)*
- No minimum tmux version recorded for `display-message -l`, on which the whole injection
  argument rests. *(Security)*
- ARCHITECTURE.md's `plan` cell value `records/2026-08-09-1116-tmux-seek` differs from the
  *superseded* design snapshot `grill/2026-08-09-1116-tmux-seek.org` by a directory prefix
  and an extension. agy misread one as the other; a human skimming can too.
- No ROI or stop check recorded: eleven grills, four ADRs and two review rounds for one
  personal keybinding. *(Context)*

---

## Dimensions skipped at triage

None.

## Reviewers returning NOTHING TO FLAG

None. All six returned findings.

## Triage challenges

The agy reviewer had no skips to audit. Two of its three blockers are surfaced above
(items 1 and the grain-composite bullet). The third is **rejected on verification**:

> "ARCHITECTURE.md's `plan` cell points to the outdated 1116 snapshot instead of this map,
> directing implementers to a superseded, broken design." — agy/Gemini, `blocker`

t11 sets the cell to `records/2026-08-09-1116-tmux-seek`, which is the records *directory*
containing this map, not `grill/2026-08-09-1116-tmux-seek.org` (the superseded design
interview). agy could not see the filesystem and the two strings are nearly identical.
Downgraded to the naming-collision nit above. This is the expected cost of the text-only
independent read, and it does not discount its other two findings, both of which verified.

## Disagreements between reviewers

- **wl-copy silence on `W`/`L`**: Operability tagged `blocker`, Resilience `should-fix`.
  Merged at `blocker` — the decision rests on a premise (message ordering is forced) that
  is false, since seek controls the ordering.
- **`set-buffer --`**: Security tagged `blocker`, Resilience `nit`. Merged at `blocker` —
  the existing thumbs commands carry `--`, so dropping it is a silent regression, not a
  theoretical edge.
- **t8's deferred branch choice**: agy `blocker`, no Claude reviewer flagged it. Downgraded
  to `should-fix` after reading t8's rebuttal, which agy also had but discounted.
</content>

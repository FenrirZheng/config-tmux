# Destination review — tmux-seek map, 2026-08-09

Run by `/review-plan` as the wayfinder Destination-reached gate on
[`tmux-seek.org`](../tmux-seek.org).

**Coverage** — 5 reviewers dispatched: Architecture & Design → agy/Gemini
(independent-model read); Context & Alignment, Resilience & Edge Cases,
Operability & Observability, Execution & Milestones → read-only Claude subagents.
1 dimension skipped at triage (Security & Compliance); skips audited by the agy
reviewer, which returned `TRIAGE-AUDIT: OK`. 0 reviewers failed.

**Focus applied** — decisions mutually consistent; no decision resting on a premise a
later decision overturned; route complete relative to the Destination. Contradiction or
gap = blocker.

**Verdict — the Destination was NOT reached.** Fifteen blockers survived the
verification gate; several are contradictions internal to the map, and one class
(unresolved open items excluded from `Not yet specified`) means an implementer still has
decisions to make, which is precisely what the Destination claims is finished.

---

## Fix these first

### 1. The map declares completeness that the record contradicts

`Not yet specified` is empty and the destination was announced, yet the four grill
snapshots carry at least six live open items: whitespace-only / trailing-space grain,
`run-shell` timing, which config file the bindings live in, branch synchronisation, the
parent-bootstrap step, and the origin-stamp false negative. Flagged independently by
four of the five reviewers.

This is the framing error that spawns blockers 5, 7, 8 and 10 below — every one of them
is an open item that was never promoted. Fixing it means the destination is not reached:
the items graduate back into `Not yet specified` or into new tickets.

### 2. The clipboard half is assigned to nobody

Two documents promise it and none of the later ones carry it:

- The design snapshot's decision 3 says seek takes over all three thumbs outputs —
  "cursor jump, `wl-copy` + `set-buffer`, and `to-claude`".
- ADR-0001's drivers list `tmux set-buffer` as one of the three.
- ADR-0002's decision text says only "pipe it to `wl-copy`". `set-buffer` is gone.
- t3's per-key table reports `→ CLIP` for `w`/`l` and "to-claude speaks" for `W`/`L`,
  and never says who clipboards on the `W`/`L` path.

Verified against the source: `tmux.conf:57` and `:68` both do `wl-copy && tmux
set-buffer`, so dropping `set-buffer` regresses `prefix ]` (advertised in `cheat.txt`).
The Resilience reviewer went further with code evidence — `to-claude --ref` does not copy
to the clipboard (only the `--paste` path does) — so as specified, `W` delivers to Claude
and clipboards nothing at all. That is the exact regression `tmux.conf:63-66` documents
having already been fixed once.

### 3. The origin-stamp contract has four defects in one mechanism

- **Consumed before it can be read.** Both the map and the 1218 snapshot say the stamp
  is "consumed with `set -p -u` and consumed *before* seek acts". Read literally the
  unset precedes the read, leaving nothing to compare the cursor against. Intent was
  read → unset → act; the text does not say that. *(agy/Gemini)*
- **Cannot distinguish "no search" from "no match".** Both leave cursor == stamp —
  measured: a non-matching query leaves the cursor unmoved — yet the message set
  specifies two distinct strings for them.
- **A cancelled search reads as live.** Measured in the effort itself: Escape leaves the
  cursor on the match (x=8 y=20 before and after). So after cancelling, `w` grabs the
  abandoned match.
- **One-shot makes the keys single-use.** A second output key after the same search
  finds no stamp, silently degrades to grabber grain, and for a multi-token query
  returns only the first word.

Four defects in one mechanism is a redesign pass, not four patches.

---

## Remaining blockers

4. **Alt-screen warning contradiction.** t2's Resolution says seek appends the warning to
   its `display-message`; t3 then assigns `W`/`L` messaging to to-claude, which knows
   nothing about `alternate_on`. The map never records the narrowing. The reasoning that
   makes it acceptable (the prompt label already warned) lives only in the 1304 snapshot.
   *(agy/Gemini — independent-model read)*

5. **Whitespace-only and trailing-space query grain undefined.** Carried through three
   grills, each deferring to the next; still unspecified. An implementer must invent it.
   All four Claude reviewers flagged it.

6. **`→ CLIP <text>` passes raw pane text through `display-message`, which expands
   formats.** Verified: `tmux display-message -p 'literal:#{pane_pid}'` prints
   `literal:536549`. `tools/ARCHITECTURE.md:121` already mandates that anything derived
   from tool arguments goes through `sanitize_format()`, and `tmuxlib` ships that helper
   at line 371 with a test. The spec never mentions either. Truncation for line grain is
   also unspecified.

7. **`run-shell` timing unverified with no fallback.** seek issues `send-keys -X
   cursor-left` back into the same pane from inside a `run-shell`. Correction: the
   `DoubleClick1Pane` binding cited as precedent is a **tmux default, not a repo
   binding** — it is absent from both tmux.conf and claude.conf. The precedent still
   shows tmux inserting `run-shell -d 0.3` there, but the map's framing is wrong.

8. **Which config file the bindings live in is undecided.** tmux.conf and claude.conf are
   not interchangeable here: claude.conf is sourced from a *queued* `run-shell` at
   `tmux.conf:128` precisely because load ordering is load-bearing in this repo.

9. **No failure contract for `wl-copy` or `to-claude`.** ADR-0002 declares a hard
   dependency on wl-copy; nothing says what happens when it is missing or fails. Combined
   with the exit-0 convention and no message, the clipboard half fails silently.
   Separately, `to-claude` exits 1 on "no marked pane and no claude pane", so seek must
   be told to swallow that code or it becomes a tmux error popup.

10. **thumbs is deleted with no rollback path.** The map says remove the `@plugin` line,
    the options, the `run-shell`, and delete `plugins/tmux-thumbs/`. That directory is
    gitignored, so the deletion is not recoverable from this repo's history, and reverting
    only tmux.conf would point `run-shell` at a missing directory. The restore path (TPM
    `prefix I` plus `tmux-thumbs-install.sh`) is never recorded.

11. **Pinning `status-keys` as specified would be silently overwritten.** t4's rationale
    says neither `mode-keys` nor `status-keys` is set anywhere. Verified false in effect:
    `plugins/tmux-sensible/sensible.tmux:117` runs `tmux set-option -g status-keys
    emacs`, and TPM executes at `tmux.conf:47`. A plain `set -g status-keys emacs` placed
    before that line loses. `mode-keys` genuinely is underived — half the claim holds.

12. **The path-shaped test is prose, not the regexp t3's Question asked for.** The
    Question says "and what regexp it uses"; the Resolution answers with "a path
    containing a slash, or `file.ext:123`, or a bare `name.ext`". Whether `v0.15.1`,
    `1.5` or `..` route to `--ref` is left to the implementer.

13. **No artifact holds the specification the Destination promises.** Every other tool in
    the workspace has a design doc in `plans/` — ten of them — and
    `tools/ARCHITECTURE.md`'s Crates table has a `plan` column keyed to those filenames.
    seek's decisions are spread across one map, four grill snapshots and two ADRs, with
    no consolidated document and nothing to put in that cell.

14. **`v`'s only failure mode is silence.** The per-key table says `seek back` reports
    nothing; the message set specifies `seek: no search — press prefix Space first`. As
    written, `v` on a dead search is indistinguishable from the unbuilt-binary silence
    that the same ticket calls the unacceptable failure mode.

15. **Scope: t4 reaches into the `$HOME` repo.** Adding a bootstrap step to `~/CLAUDE.md`
    is outside "build the crate and rewire tmux.conf". Related correction: the claim that
    the bootstrap never mentions `~/.tmux/tools` is true of `~/CLAUDE.md` but
    `runbooks/to-emacs.md`'s Install section already asserts "Steps 1–2 are part of the
    home repo's normal bootstrap", step 2 being `cd ~/.tmux/tools && cargo build
    --release`. The two documents contradict each other today.

---

## Should-fix

- Re-pressing an output key after `v` silently changes the answer for multi-token
  queries; state whether re-pressing is supported.
- The reload after building is named as a caveat but never as a command.
  `runbooks/to-emacs.md` step 3 already gives it: `tmux source-file ~/.tmux/tmux.conf`.
- The load-time guard is one-directional — it covers unbuilt → stubs, not a later
  `cargo clean` leaving live keys pointing at a vanished binary.
- The origin-stamp false-negative rationale assumes entry at the pane bottom, but
  `prefix C-t` (tmux.conf:19) then `prefix Space` starts mid-screen on real text, where a
  match on the origin cell is no longer improbable.
- ADR-0002 leans on cheat.txt's "all three land in wl-copy" invariant while t4 rewrites
  the entries that sentence counts; cheat.txt line 33 is not in the rewrite scope.
- No unit-test plan, though all sibling crates carry `#[cfg(test)]` and seek's grain
  splitter and path classifier are pure functions.
- No execution checklist: eight artifacts across two repos, no order, no per-artifact
  acceptance criteria.
- Big-bang cutover was chosen without recording it as a rollout decision — the one
  parallel-run option was declined for feature loss, not rollout risk.
- The removal of a working plugin has no stated driver beyond a circular one; ROI on a
  Rust crate plus runbook plus doc edits cannot be judged without it.
- Line grain's `--ref` / `--paste` routing and `v`'s cursor mechanism are specified in the
  1116 design snapshot but not restated in t3; agy flagged both as gaps because it could
  only see the map. *(downgraded from agy's `should-fix` after verification)*

## Nits

- README.md indexes runbooks; `runbooks/seek.md` needs an index line.
- Counts in the record are wrong: the workspace has **nine** binary crates plus
  `tmuxlib`, not ten; and `target/release` is referenced on 15 lines across claude.conf
  and tmux.conf, not the "eleven bindings" a snapshot claims.
- The runbook's Troubleshooting section is specified by shape only; the failure messages
  are never mapped to causes.

---

## Dimensions skipped at triage

**Security & Compliance** — skipped on the grounds that this is a single-user personal
tool with no network surface, credentials, PII or audit regime, and that the one trust
boundary (terminal text flowing into an AI agent's input) is pre-existing `copy-mode Y`
and thumbs behaviour that the map leaves unchanged.

The agy reviewer audited this skip and returned `TRIAGE-AUDIT: OK`. **It should not have
been skipped anyway.** Blocker 6 — raw pane text reaching a format-expanding
`display-message` with a `sanitize_format()` convention already on the books — is an
injection-shaped finding, and it surfaced from the Operability lens by luck rather than
by design. The skip reason was too narrow: it considered where the text *goes* and not
what the text *is*.

## Reviewers returning NOTHING TO FLAG

None. All five returned findings.

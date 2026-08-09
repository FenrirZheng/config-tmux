# Review — runbooks/seek.md, 2026-08-09 19:43

Run by `/review-plan` to discharge the `review(agent): /review-plan` line on task T9 of
[the seek build-and-cutover checklist](../../../../records/2026-08-09-1116-tmux-seek/seek-build-and-cutover/2026-08-09-1858_TASK.org).

**Coverage** — 1 reviewer dispatched: Operability & Observability → agy/Gemini
(independent-model read). 5 dimensions skipped at triage; skips audited by the agy
reviewer, which returned `TRIAGE-AUDIT: OK`. 0 reviewers failed. **Fan-out narrowed at the
author's request** ("keep the fan-out small; this is a one-page runbook for a personal
tool") — both anchored questions land in the same lens, so one reviewer covers the whole
ask.

**Focus applied** — two anchored questions: (1) does every message in the map's Message
set have a Troubleshooting row naming cause and fix? (2) is the rollback recipe executable
under stress, every step a written command?

**Verdict — both questions answered; one blocker, one should-fix, all fixed in place.**

## Triage table

| # | Dimension | discuss / skip | reason |
|---|---|---|---|
| 1 | Context & Alignment | skip | a runbook states no problem definition or ROI — that lives in the map |
| 2 | Architecture & Design | skip | documents decisions made elsewhere; the design surface is the map's Current contract |
| 3 | Resilience & Edge Cases | skip | its failure surface *is* the Troubleshooting table — routed into Operability as question 1 |
| 4 | Operability & Observability | discuss | both anchored questions land here |
| 5 | Security & Compliance | skip | no auth, no secrets, no network surface, single-user machine |
| 6 | Execution & Milestones | skip | a runbook has no milestones or test-coverage strategy |

## Blockers

**1. The rollback recipe's last two steps were keypresses, not commands.**
`prefix I` (TPM re-clone) and `prefix Space` (first-press build) are actions, not
pasteable commands — which fails the author's own stated criterion for question 2.
*[agy/Gemini · independent-model read]*

Verified before fixing, and agy's suggested fix was **not** taken. It proposed
`tmux send-keys`, which is fragile. Reading `tmux-thumbs-install.sh` shows it blocks on
two `read -rs -n 1` prompts and a `select` menu, so it can never be a pasteable step —
but its *Compile* branch is just `cargo build --release --target-dir=target`, so the
installer can be bypassed entirely. Both scriptable entry points confirmed present:
`~/.tmux/plugins/tpm/bin/install_plugins` and the plugin's own Cargo project.

**Fixed** — the recipe is now six numbered steps, every one a pasteable command, with a
note saying why the installer and `prefix I` are deliberately avoided.

## Should-fix

**2. `<cutover-commit>` was a placeholder with no way to resolve it** — an operator mid-
rollback would stall on finding the hash. *[agy/Gemini · independent-model read]*

**Fixed** — step 1 is now `git -C ~/.tmux log --oneline -S '@plugin' -- tmux.conf | head -3`,
which finds the commit that removed the `@plugin` line.

**3. Verify row 7 told the operator to `sudo mv /usr/bin/wl-copy /tmp/` with "restore
afterwards" as prose.** *(main Claude — mine, not a sub-reviewer's.)* A destructive
instruction whose undo is a description rather than a command is the same defect as
finding 1, in the Verify section instead of the Rollback section, and it leaves the
machine without a working clipboard if the operator is interrupted.

**Fixed** — row 7 now points at a paired block that breaks and restores in one
copy-pasteable unit, ending with `command -v wl-copy` to confirm the restore.

## Nits

Three, all from agy, all **cleared rather than actioned** — it identified the messages
with no Troubleshooting row and judged each omission defensible, which is the correct
answer to question 1:

- `→ CLIP <text≤48>` — success, nothing to troubleshoot.
- `→ CLAUDE <target>` — success, nothing to troubleshoot.
- `(visible screen only)` — informational, and it has its own section
  ("Alt-screen panes see one screen only").

Confirmed independently: every *failure* message in the map's Message set has a row —
`⚠ wl-copy failed → BUFFER`, `⚠ wl-copy AND set-buffer failed`, the `W`/`L` wl-copy
failure, `seek: to-claude not built`, `seek: nothing under the cursor`, `seek: not built`.
**Question 1 passes.**

## Dimensions skipped at triage

Five (see the triage table). The agy reviewer audited them and returned
`TRIAGE-AUDIT: OK`.

## Reviewers returning NOTHING TO FLAG

None — the single reviewer returned findings.

---

Snapshot: `runbooks/.review-plan/seek.md/20260809-1943/`. Note this `plan.snapshot` is the
runbook **after** the three fixes above were applied, not the version the reviewer saw —
so a later `delta` run diffs against the corrected text, which is what you want.

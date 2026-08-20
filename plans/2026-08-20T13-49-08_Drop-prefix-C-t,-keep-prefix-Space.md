---
captured: 2026-08-20 13:49
session: 1c4f917a-7ac7-4f90-aa68-ec2a5f3b642d
project_dir: /home/fenrir/.tmux
cwd: /home/fenrir/.tmux
transcript: /home/fenrir/.claude/projects/-home-fenrir--tmux/1c4f917a-7ac7-4f90-aa68-ec2a5f3b642d.jsonl
source: ExitPlanMode (PostToolUse hook)
plan_source: /home/fenrir/.claude/plans/ctrl-b-space-parallel-pnueli.md
---

# Drop `prefix C-t`, keep `prefix Space`

## Context

`~/.tmux/` currently has two prefix keys that both enter copy-mode:

- `prefix C-t` → `copy-mode` (plain entry) — [`tmux.conf:18-19`](/home/fenrir/.tmux/tmux.conf)
- `prefix Space` → `copy-mode` + tmux's incremental search prompt (seek) — [`claude.conf:123`](/home/fenrir/.tmux/claude.conf)

The overlap is real and the user wants one key, not two: keep `prefix Space`, remove
`prefix C-t`.

Nothing is lost by the removal. Verified live with `tmux list-keys -T prefix`:

```
bind-key -T prefix [      copy-mode      <- tmux default, untouched by this repo
bind-key -T prefix C-t    copy-mode      <- the duplicate being removed
bind-key -T prefix Space  if-shell ...   <- seek, being kept as-is
```

`prefix [` stays as the plain copy-mode entry, so the only thing that disappears is the
second alias. `prefix Space` needs no work — it is already bound the way the user wants it.

## Changes

### 1. `~/.tmux/tmux.conf` (lines 18-19)

Replace the binding with an explicit unbind plus the reason:

```tmux
# prefix + C-t used to be a second copy-mode entry. Removed 2026-08-20: it
# duplicated `prefix [' (tmux's own, still bound) and `prefix Space' (seek,
# claude.conf) with nothing of its own. `unbind' not just a deletion, because
# `source-file' does NOT remove a stale binding from a running server
# (measured — see runbooks/seek.md, "source-file does not unbind").
unbind C-t
```

`unbind` rather than plain deletion follows the existing `unbind C-a` precedent at
`tmux.conf:15` and makes a re-source idempotent on the already-running server.

### 2. `~/.tmux/cheat.txt` (line 12)

The cheat sheet advertises the key being removed. Rewrite the first COPY & GRAB row so
`prefix [` becomes the named plain entry:

```
  COPY & GRAB  prefix [        copy mode (leave with q)
```

Keep the existing column grid (key column at col 16, description at col 32) and the ≤78
char width — the popup is `display-popup -w 78 -h 34` (`tmux.conf:33`). Lines 13-20 of the
sheet (seek, `w`/`l`, `prefix ]`, `prefix E`) are unaffected.

### Explicitly NOT touched

- `claude.conf` — the `prefix Space` seek binding is already what the user wants.
- `records/2026-08-09-1116-tmux-seek/**` — historical design records of a finished effort;
  they describe what was true then, and the other `C-t` hits in the repo are all in there.
- `tools/atlas/*` — the atlas covers the Rust crates under `tools/`, not tmux key tables;
  no node claims anything about `C-t`.
- `runbooks/seek.md`, `README.md`, `CONTEXT.org`, `docs/adr/*` — grepped, none mention `C-t`.

## Verification

```bash
tmux source-file ~/.tmux.conf

# C-t gone, Space and [ both intact:
tmux list-keys -T prefix | grep -E '(-T prefix (C-t|Space|\[))'
#   expect: no C-t row; the Space if-shell row; `[  copy-mode`
```

Then, by hand in a pane:

1. `prefix C-t` → nothing happens (no copy-mode, no error).
2. `prefix [` → enters copy-mode; `q` leaves.
3. `prefix Space` → copy-mode + `(seek)` prompt; type text, Enter lands on the match, `w`
   grabs the token. (Full procedure: [`runbooks/seek.md`](/home/fenrir/.tmux/runbooks/seek.md).)
4. `prefix ?` → cheat popup renders, COPY & GRAB row now reads `prefix [`, no wrapped lines.

## Commit

One commit in `~/.tmux/` covering both files (config + its cheat sheet are the same
change, per the repo's "don't split a feature's halves" convention):

```
tmux: drop prefix C-t, prefix [ / prefix Space cover copy-mode
```

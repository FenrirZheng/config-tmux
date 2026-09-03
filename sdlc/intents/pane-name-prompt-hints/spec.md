# Spec: pane name prompt hints

Author: claude (agent)
Created: 2026-09-03
Status: accepted
Accepted: 2026-09-03

Reads [`intent.md`](intent.md) (`Status: accepted`, 2026-09-03). Every measurement
below was taken 2026-09-03 on tmux 3.5a and fzf 0.60 (`tmux -V`, `fzf --version`)
unless marked `[未驗證]`.

## Requirements

Numbered so `plan.md` and the review can cite them.

- R1 **List first.** `prefix g` opens a popup that lists every pane on the server,
  all sessions, one row per pane: `session:window.pane`, the running command, the
  pane title. The pane id is carried on the row but not displayed. Scope equals the
  current binding's `-a`.
- R2 **Type to filter.** Keystrokes filter the rows on the title column only,
  case-insensitive substring, the same match rule as today's `#{mi:*x*,…}`. The
  command and location columns are shown but not matched, so typing `bash` does not
  match every shell.
- R3 **One confirming key.** `Enter` switches the attached client to the highlighted
  pane: session, window and pane in one move. The cursor opens on the first row, so
  a fragment that narrows the list to one pane needs no `Down`. This is the intent's
  success criterion: `prefix g`, read, type, `Enter`, target active.
- R4 **Fail closed on a miss.** With zero matching rows, `Enter` moves nothing, prints
  `pane-jump: no pane title matches` through `tmux display-message`, and the helper
  exits non-zero. No full tree, no silent success.
- R5 **Cancel is silent.** `Esc` or `C-c` closes the popup, moves nothing, prints
  nothing.
- R6 **Titles are read, never written.** The helper contains no `select-pane -T` and
  no `rename-window`. Claude-pane readers (`talk ping`, `cc-attend`, `cc-fleet`) are
  unaffected.
- R7 **Missing fzf fails closed.** If `fzf` is not on `PATH`, the helper prints
  `pane-jump: fzf not installed` via `display-message` and exits 1. fzf is already a
  runtime dependency of `prefix S` (`cc-fleet`), so this adds no new dependency class.
- R8 **Same key, old binding replaced.** The `command-prompt` + `choose-tree` line in
  `tmux.conf` is replaced, not kept beside the new one. Because the key is rebound
  rather than unbound, `tmux source-file ~/.tmux.conf` is enough to install it on a
  running server; the stale-binding hazard noted at `tmux.conf:18-23` applies only to
  a removed key.
- R9 **Docs move with the key.** `cheat.txt:7` loses `(Down Down, then Enter)` and
  stays one line, at most 81 cells wide, so the `prefix ?` popup height at
  `tmux.conf:47` does not change. `README.md:7` names the popup instead of the
  filtered tree. `runbooks/switch-pane-by-name.md` marks the popup as shipped, keeps
  Option A as the no-helper fallback, and its Verify and Troubleshoot sections
  describe the new behaviour.
- R10 **Verified on a throwaway server.** The build proves R1 to R5 and R7 on a nested
  `tmux -L` server with panes titled `alpha-build`, `beta-server`, `gamma-notes`, per
  the runbook's testing note; the user's live server is only read from. The bound
  key on the live server stays `[未驗證]` until the user presses it; the spec does not
  claim otherwise.

## Design

### Shape: popup running fzf, then one `switch-client`

Plain tmux cannot do R1 and R3 together. `command-prompt` is a one-line prompt with no
list; `choose-tree -f` lists but ignores an empty filter (R4 fails) and opens on the
session row (R3 fails). `display-popup -E` around an fzf picker does both, and the repo
already ships that shape for Claude panes at `claude.conf:89` (`prefix S`). This is the
"fzf popup that filters as you type" answer to the intent's first open question.

The intent allows a helper script and forbids a new Rust crate unless tmux alone
cannot do it. A bash helper is enough: the filter logic is fzf's, and the jump is one
tmux command.

### Helper: `bin/pane-jump`

New tracked file, executable, bash with `set -euo pipefail`. Three entry points, so
the two halves are testable without a TUI, the same split `cc-fleet rows` uses:

| invocation | does |
|---|---|
| `pane-jump` | rows → fzf → jump; the bound form |
| `pane-jump rows` | prints the rows only, tab-separated: `pane_id`, `session:window.pane`, `command`, `title` |
| `pane-jump jump <pane_id>` | the guarded `switch-client`; empty or missing id exits 2 with a message |

Rows come from one call:

```bash
tmux list-panes -a -F '#{pane_id}	#{session_name}:#{window_index}.#{pane_index}	#{pane_current_command}	#{pane_title}'
```

Measured on the live server: tab-separated, one line per pane, titles with the `✳`
prefix pass through intact (`cat -A` shows `M-bM-^\M-3`, the UTF-8 bytes).

fzf invocation, deciding R2 and R3:

```bash
fzf --delimiter='\t' --with-nth=2.. --nth=4 --exact -i \
    --layout=reverse --prompt='pane name: ' --no-multi
```

- `--with-nth=2..` hides the id column, `--nth=4` restricts matching to the title
  column (R2).
- `--exact -i` is case-insensitive substring, the current rule. fzf's default fuzzy
  match would also hit `cs` against `Claude Code Skills`; the intent asked for
  "as today", so exact wins. Flagged below as a taste choice.
- `--layout=reverse` puts the prompt at the top and the first match directly under
  it, the row `Enter` selects (R3).

Exit handling, deciding R4 and R5. Measured with `--filter`: a match exits 0 and
prints the row; no match exits 1 and prints nothing. Interactive fzf documents 130
for `Esc` / `C-c`:

| fzf exit | selection | helper does |
|---|---|---|
| 0 | one row | cut field 1, `pane-jump jump <id>` |
| 1 | none | `display-message 'pane-jump: no pane title matches'`, exit 1 |
| 130 | none | exit 130, no message |

The jump is a single `tmux switch-client -t "$id"`. The runbook measured it on
2026-09-03: `switch-client -t %0` changes session, window and pane in one call, and
`switch-client -t ""` exits 0 doing nothing, which is why `jump` refuses an empty
argument before calling tmux. That a `switch-client` issued from inside
`display-popup -E` moves the client that opened the popup is proven by `cc-fleet`,
which calls `focus_pane(None, id)` from the same position
(`tools/cc-fleet/src/main.rs:68`, `tools/tmuxlib/src/lib.rs:339-347`).

### Binding

In `tmux.conf`, replacing lines 109-110 and their comment block, staying in
`tmux.conf` rather than `claude.conf` because the key is not Claude-specific:

```tmux
bind -T prefix g display-popup -E -w 80% -h 60% -T ' pane name ' "~/.tmux/bin/pane-jump"
```

Size copied from the project picker at `claude.conf:102`. Absolute path, the shape
every other popup in the repo uses.

### Not included, with reason

- **Auto-jump on a unique match.** fzf 0.53+ can accept as soon as one row remains.
  The intent's success criterion names "one confirming key", and an auto-jump would
  fire on the first letter that happens to be unique; left out, and left open below.
- **Preview of the target pane.** `cc-fleet` shows one. It is a second feature, not
  part of "hint, type, jump"; left out.
- **Current-pane exclusion or marking.** The current pane is a legal target; a
  no-op jump is harmless.
- **Matching on the command column.** Excluded by R2; the column is display only.

### Files that change

| file | change |
|---|---|
| `bin/pane-jump` | new, executable |
| `tmux.conf` | `prefix g` binding and its comment |
| `cheat.txt` | line 7 text, same line count and width budget |
| `README.md` | line 7 entry |
| `runbooks/switch-pane-by-name.md` | shipped recipe, Verify, Troubleshoot, Roll back |

Untouched: `claude.conf`, `tools/`, `plugins/`, `.gitignore` (a new tracked `bin/`
needs no rule).

## Flagged concerns

Single-operator repo: the policy owner for each is the user, fenrir. Each is a
decision the reviewer settles before the plan is written.

1. **New top-level `bin/`.** The repo has `plugins/` for bash plugins and `tools/` for
   the cargo workspace, and no home for a standalone script. `bin/pane-jump` is the
   smallest addition; the alternative is `plugins/tmux-pane-jump/` mirroring
   `tmux-ace-window`, which costs a loader and a `.tmux` entry file for one script.
2. **fzf on a non-Claude key.** `prefix g` now depends on fzf; today it depends on
   tmux only. R7 makes the failure visible. The same dependency already gates
   `prefix S`, `prefix C` pickers.
3. **Exact substring, not fuzzy.** Chosen to match the current rule. Switching to
   fuzzy is one flag (`--exact` removed) and can be decided at review.
4. **Runbook scope.** The runbook is 300 lines built around `choose-tree`; R9 rewrites
   its recipe and tables rather than appending a third option, so its "what tmux
   cannot do" measurements stay and its shipped recipe changes. A reviewer who wants
   the old Option A text preserved verbatim should say so before the plan.

## Open questions carried from intent.md

- **Presentation** — answered: fzf popup, filtering as you type, the `cc-fleet`
  shape. Not `display-message` before a prompt, not a pre-positioned `choose-tree`.
- **Ambiguity on several matches** — answered: the list is already on screen and
  narrows as you type; `Enter` takes the highlighted row, `Down` picks another. No
  separate "unique jumps, multi lists" branch is needed because the list is always
  shown. Whether a unique remaining match should also auto-accept stays open; the
  spec ships without it (see Not included).
- **Match rule** — answered: substring, case-insensitive, title column only (R2).
- **A miss** — answered: empty list while typing; `Enter` on it prints a message and
  closes, never the full tree (R4).

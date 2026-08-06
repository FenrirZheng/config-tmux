# Pane tape: flight recorder past the alt-screen, with a /tape reading skill

## Problem

Three existing capture paths all stop at the same wall — the **alternate screen**:

- [tmux.conf](../tmux.conf) itself documents it (mouse-mode comment, lines 7–9):
  full-screen TUIs like Claude Code, vim, and `less` have *no tmux scrollback*;
  the wheel is forwarded to the app precisely because tmux has nothing to scroll.
- `talk read` / `talk read-since` (the inter-pane capture path in
  `~/.claude/skills/talk/SKILL.md`) declare the same caveat: "`read` only sees
  tmux scrollback"; anything an alt-screen TUI redrew over is gone.
- `capture-pane` (what monitor-tmux-pane and fleet-style popups use) sees only
  the currently visible screen of an alt-screen app.

When it bites: an overnight `claude -p` run or an installer TUI finishes (or
wedges) and there is **no record** of what happened between the last glance and
now. Worse, the most expensive supervision failure — an agent stuck rerunning
the same failing test for 20 minutes — is invisible to `talk ping` (busy is
busy) and to `capture-pane` (the visible screen looks like normal progress).

## Design

### User-visible behavior

1. **`prefix + T` toggles the tape** on the current pane. On: a `●REC` light
   appears in the pane border and everything the pane prints from that moment
   is appended, ANSI-stripped, to `~/.cache/tmux-tape/<pane_id>.log` (e.g.
   `%12.log`). Off: the light disappears, a timestamped `── tape off ──`
   divider is written. Recording survives the pane's apps switching in and out
   of the alt-screen — `pipe-pane` taps the byte stream, not the screen.
2. **A structured ticker runs automatically for every Claude Code pane** — no
   toggle needed. A `PostToolUse` `*` hook appends one line per tool call
   (`HH:MM Bash go test ./pkg/...`) to `/tmp/claude-progress/<pane_id>.log`;
   a `Stop` hook appends a `── idle ──` divider. This is the *semantic* tape:
   grep-friendly, tiny, and immune to TUI redraw noise.
3. **`/tape` skill** teaches any Claude (this pane or a sibling) to read either
   log for a given `%id` with `tail`/`rg` slices — mirroring `talk read-since`'s
   marker discipline so only the relevant slice enters context.
4. **`prefix + W`** (optional, MVP step 6) pops up a live `tail -f` of the
   *active* pane's ticker — compose with `prefix + o` (ace-window) to inspect
   any pane in two keystrokes.

### Components

| piece | lives in | repo |
| --- | --- | --- |
| `bind T` / `bind W` + scripts `scripts/tape.sh`, `scripts/tape-strip.sh` | [tmux.conf](../tmux.conf), `~/.tmux/scripts/` | this repo (`config-tmux`) |
| ticker hook `tape-ticker.sh` + `settings.json` wiring | `~/.claude/hooks/` | `.claude` submodule |
| `/tape` skill | `~/.claude/skills/tape/SKILL.md` | `.claude` submodule |
| raw tape data | `~/.cache/tmux-tape/` (persistent) | untracked (hidden by `showUntrackedFiles=no`) |
| ticker data | `/tmp/claude-progress/` (ephemeral, gone at reboot) | untracked |

Verified on this machine: tmux 3.5a, `#{pane_pipe}` format works, GNU sed 4.9
supports `-u`, `jq` and `fzf` installed. Note the tokyo-night theme sets
`pane-border-status off` at load (`plugins/tokyo-night-tmux/tokyo-night.tmux:27`),
so the REC light must flip border status per-window at toggle time — a global
`pane-border-format` alone would never render.

## Implementation sketch

### tmux.conf additions (before the theme block is fine; binds are order-free)

```tmux
# Pane tape: flight recorder past the alt-screen.  prefix+T toggle, ●REC in
# border while recording.  Raw log: ~/.cache/tmux-tape/<pane_id>.log
bind T run-shell "~/.tmux/scripts/tape.sh toggle '#{pane_id}'"
# Live ticker of the ACTIVE pane's Claude tool calls (prefix+o first to aim).
bind W display-popup -w 80% -h 60% -E \
  "tail -n 200 -f /tmp/claude-progress/'#{pane_id}'.log 2>/dev/null || echo 'no ticker for #{pane_id} (not a Claude pane?)'"
```

### `~/.tmux/scripts/tape.sh`

```sh
#!/bin/sh
# tape.sh toggle <pane_id> — start/stop pipe-pane recorder + REC border light.
set -eu
dir="$HOME/.cache/tmux-tape"; mkdir -p "$dir"
pane="$2"; log="$dir/$pane.log"
case "$1" in
toggle)
  if [ "$(tmux display -p -t "$pane" '#{pane_pipe}')" = "1" ]; then
    tmux pipe-pane -t "$pane"                       # no command = close pipe
    printf '%s ── tape off ──\n' "$(date '+%F %T')" >> "$log"
  else
    printf '%s ── tape on ──\n' "$(date '+%F %T')" >> "$log"
    tmux pipe-pane -t "$pane" -o \
      "exec $HOME/.tmux/scripts/tape-strip.sh >> '$log'"
  fi
  # REC light: border status on for this window iff any pane here is piped.
  if tmux list-panes -t "$pane" -F '#{pane_pipe}' | grep -q 1; then
    tmux set -w -t "$pane" pane-border-status top
    tmux set -w -t "$pane" pane-border-format \
      '#{?pane_pipe,#[fg=red#,bold] ●REC #[default],}#{pane_index} #{pane_title}'
  else
    tmux set -w -t "$pane" -u pane-border-status
    tmux set -w -t "$pane" -u pane-border-format
  fi
  ;;
esac
```

### `~/.tmux/scripts/tape-strip.sh`

```sh
#!/bin/sh
# Unbuffered ANSI/OSC strip for pipe-pane output.  CSI, OSC (BEL- or
# ST-terminated), charset selects; CR→newline so redrawn lines stay greppable.
exec sed -u \
  -e 's/\x1b\[[0-9;?<>=!]*[ -\/]*[@-~]//g' \
  -e 's/\x1b\][^\x07\x1b]*\(\x07\|\x1b\\\)//g' \
  -e 's/\x1b[()][0-9A-B]//g' \
  -e 's/\r$//' -e 's/\r/\n/g'
```

### Ticker hook `~/.claude/hooks/tape-ticker.sh` (in the `.claude` submodule)

```bash
#!/bin/bash
# PostToolUse '*' / Stop — one line per tool call into /tmp/claude-progress/<pane>.log
set -uo pipefail
[[ -n "${TMUX_PANE:-}" ]] || exit 0          # not in tmux → no ticker, not an error
dir=/tmp/claude-progress; mkdir -p "$dir"
log="$dir/${TMUX_PANE}.log"
if [[ "${1:-}" == "--stop" ]]; then
  printf '%s ── idle ──\n' "$(date +%H:%M)" >> "$log"; exit 0
fi
input="$(cat)"
line="$(jq -r '
  def clip(n): tostring | if length > n then .[:n] + "…" else . end;
  (.tool_name // "?") + " " +
  ((.tool_input // {}) |
   (.command // .file_path // .pattern // .url // .description // .skill //
    (keys | join(","))) | clip(90))' <<<"$input")" || exit 0
printf '%s %s\n' "$(date +%H:%M)" "$line" >> "$log"
exit 0
```

`settings.json` wiring (append to the existing `PostToolUse` array; the `Stop`
key is **new** — settings.json currently has no `Stop` block, despite the
global CLAUDE.md hook table listing `cross-pane-enforce.sh` there; resolve that
drift while editing):

```json
"PostToolUse": [
  { "matcher": "ExitPlanMode", "hooks": [ { "type": "command", "command": "~/.claude/hooks/plan-capture.sh" } ] },
  { "matcher": "*",            "hooks": [ { "type": "command", "command": "~/.claude/hooks/tape-ticker.sh" } ] }
],
"Stop": [
  { "matcher": "", "hooks": [ { "type": "command", "command": "~/.claude/hooks/tape-ticker.sh --stop" } ] }
]
```

### `/tape` skill (`~/.claude/skills/tape/SKILL.md`) — content outline

- **Files**: raw tape `~/.cache/tmux-tape/<%id>.log` (only if `prefix+T` was
  on), ticker `/tmp/claude-progress/<%id>.log` (every Claude pane, automatic).
  Discover ids with `talk list`; check `#{pane_pipe}` to see if a tape rolls.
- **Slice discipline** (mirrors `talk read-since`): never `cat`; use
  `tail -n 100`, or anchor on a marker/divider —
  `tail -n "+$(rg -n '── tape on ──' f | tail -1 | cut -d: -f1)" f`.
- **Loop detection recipe** (the 20-minutes-of-the-same-test case):
  `cut -d' ' -f2- /tmp/claude-progress/%12.log | sort | uniq -c | sort -rn | head`
  — a count ≫ 5 on one `Bash go test …` line is the smoking gun; then
  `rg -n 'FAIL|panic|error' ~/.cache/tmux-tape/%12.log | tail -20` for the why.
- **Report format**: quote ≤ 20 lines back to the user; state which log and
  what slice was read.

## Integration with existing setup

- **talk**: `/tape` is the escape hatch `talk read`'s own caveats point at;
  skill cross-references `talk list` for id discovery and reuses its marker
  discipline. `talk ping` says *busy*; the ticker says *busy doing what*.
- **mq / monitor-tmux-pane / pair**: supervising agents that today poll
  `capture-pane` can `rg` the ticker instead — cheaper and history-complete.
  No changes to mq itself.
- **ace-window**: `prefix+o` then `prefix+W` = inspect any pane's ticker in two
  keystrokes; no separate picker needed (fzf variant possible later).
- **thumbs**: stripped tape logs keep `file.ext:123` strings intact, so
  `less` -ing a tape inside a pane still lets `prefix+Space` hint-copy them.
- **cheat.txt**: add two lines under COPY & GRAB in [cheat.txt](../cheat.txt):
  `prefix T  tape: record pane past the alt-screen (●REC)` and
  `prefix W  live ticker popup of active pane's Claude tool calls`.
- **hooks**: new `tape-ticker.sh` sits beside `talk-wrap.sh`/`plan-capture.sh`;
  commit inside `.claude`, then bless the gitlink in the `$HOME` parent repo.
- **Border format**: nothing else sets `pane-border-format` today (verified:
  tokyo-night only styles + turns status off). If a future "beacon" feature
  claims the border, merge its condition into the same format string [未驗證 —
  no beacon exists in tmux.conf yet].

## Risks & open questions

- **Alt-screen redraw noise**: even stripped, TUI frames repeat lines; the raw
  tape is greppable, not readable prose. Mitigation: the ticker is the readable
  channel for Claude panes; the tape is forensic.
- **Unbounded growth**: `~/.cache/tmux-tape/` persists. MVP: manual `rm`;
  follow-up option: toggle-on trims the log to last 5 MB (`tail -c`).
- **Hook overhead**: one `jq` fork per tool call. Negligible next to a tool
  round-trip, but the hook must `exit 0` on all failures so a broken jq filter
  never blocks tool use (hence `set -uo`, no `-e`, and `|| exit 0`).
- **Pane-id reuse**: tmux `%N` ids are unique per server-lifetime, but logs
  outlive server restarts — a new server restarts numbering and appends to old
  files. Dividers carry full dates, so slices stay disambiguable.
- **Secrets on tape**: the pipe records *everything printed*, including tokens
  a tool echoes. Logs live outside any repo and gitleaks never scans them, but
  a `/tape`-reading Claude may pull one into context. Skill must say: slice
  narrowly, never paste raw slices into commits or messages.
- **`pipe-pane` shell quoting**: the command string is run by `sh` with tmux
  format expansion; pane ids (`%12`) are safe, but keep the command minimal —
  all logic stays in `tape-strip.sh`.
- Open: should `claude-session-dispatch` auto-start the tape (`tape.sh toggle`)
  for panes it spawns? Deferred until the manual toggle proves its worth.

## MVP steps (each independently testable)

1. **`tape-strip.sh` + manual pipe**: write the strip script; test with
   `tmux pipe-pane -o "exec ~/.tmux/scripts/tape-strip.sh >> /tmp/t.log"` on a
   pane running `less` on a colored file; verify log is ANSI-free and survives
   alt-screen entry/exit. Commit (this repo).
2. **`tape.sh toggle` + `bind T`**: add the bind, toggle twice, verify
   `#{pane_pipe}` flips, dividers written, `●REC` appears/disappears and other
   windows' borders stay off. Commit with the cheat.txt lines (this repo).
3. **Ticker hook**: add `tape-ticker.sh` + `PostToolUse "*"` wiring; run any
   Claude session in tmux, confirm one line per tool call in
   `/tmp/claude-progress/$TMUX_PANE.log` and that non-tmux sessions are silent.
   Commit in `.claude`, bless gitlink in parent.
4. **Stop divider**: add the `Stop` block (resolving the cross-pane-enforce
   drift note above); confirm `── idle ──` appears when a turn ends.
5. **`/tape` skill**: write SKILL.md; test by asking a sibling Claude to
   diagnose a deliberately looped `watch date` pane from its tape. Commit in
   `.claude`, bless gitlink.
6. **`bind W` popup** (optional polish): add the bind; verify the tail follows
   live and the no-ticker fallback message shows on a shell-only pane.

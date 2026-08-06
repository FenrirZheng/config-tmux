# Fleet popup: fzf over every Claude pane, with live preview and jump

## Problem

With 3–6 Claude Code instances spread across tmux sessions, the pane you need is
never the pane you're in. Concretely:

- **`choose-tree` (prefix+w) is useless for this**: five windows all render as
  `claude` / `node`; nothing says which one is the backend repo, which is idle
  waiting for input, which is blocked on a permission prompt.
- **Cross-session is the hard case.** [ace-window](../plugins/tmux-ace-window/README.md)
  (prefix+o/O) covers panes *in the current window*; nothing covers "the Claude I
  started in session `work2` an hour ago".
- **Peeking costs a context switch.** To check whether a sibling finished, you
  currently either switch to it (losing your place, risking a keystroke landing in
  its prompt) or run `talk read %42` from your own Claude (burning your own
  transcript on its scrollback).

The friction bites hardest at the "which pane was the backend one?" moment: you
know the fleet exists, you can't name a target.

## Design

### User-visible behavior

`prefix + S` opens a `display-popup` (90% × 70%, same idiom as the existing
prefix+P / prefix+? popups in [tmux.conf](../tmux.conf)) containing an fzf list —
one row per **Claude Code pane, across all sessions**:

```
● idle   12m  work2:1.0  ~/code/backend    ✳ fix auth middleware   │ Done. Ready for review.
◐ busy    3m  main:2.1   ~/fenrir-tools    ⠹ running go test       │ Bash(go test ./…)
○ ????    —   scratch:0.0 ~/.tmux          Claude Code             │ >
```

- **Columns**: state glyph, time-in-state, `session:window.pane`, repo dir
  (basename), current activity (pane title), last non-blank output line.
- **Right side**: live preview = `tmux capture-pane -e -p` of the highlighted pane
  (last 200 lines, ANSI colors preserved). This *is* the transcript — the preview
  doubles as a read-only peek.
- **Enter**: switch the client to that pane (`select-window` + `select-pane` +
  `switch-client`). **Esc / ctrl-c**: close with zero side effects — the peek case.
  **ctrl-r**: refresh the preview (re-capture) without moving the cursor.
- Sort order: attention-needing first (`idle` before `busy`), then longest
  time-in-state first — "the one that's been waiting for you longest" is on top.

### Components

1. **`~/.tmux/scripts/claude-fleet.sh`** — builds rows, runs fzf, executes the
   jump. Pure tmux + fzf (fzf 0.60 and tmux 3.5a verified installed).
2. **One bind in [tmux.conf](../tmux.conf)** (`S` is unbound in stock tmux and by
   tmux-sensible).
3. **Claude-pane detection, two phases**:
   - **Phase 1 (MVP, zero new infra)**: the pane-title heuristic already proven in
     [`talk ping`](../../.local/bin/talk) — Claude Code sets the title to
     `✳ <task>` when idle, a braille spinner (U+2800–U+28FF) when busy,
     `Claude Code` when fresh. Panes whose title matches none of these are
     filtered out.
   - **Phase 2 (beacon hooks)**: Claude Code hooks stamp per-pane user options
     `@claude_state` / `@claude_since` / `@claude_activity` via
     `tmux set-option -p`. This adds what the title cannot carry: a distinct
     `blocked` state (Notification hook = permission prompt) and a reliable
     time-in-state. **Note: no such beacon hooks exist today** — the hooks dir
     ([~/.claude/hooks/](../../.claude/hooks/)) has talk/gotest/sudo/cross-pane
     hooks only; `rg '@claude_state' ~/.claude ~/.tmux` returns nothing. Phase 2
     is new work in the `.claude` submodule, and the fleet script must degrade
     gracefully when the options are empty (Phase 1 heuristic as fallback).

## Implementation sketch

### tmux.conf (append near the other popup binds)

```tmux
# prefix + S : Claude fleet — fzf over every Claude Code pane (all sessions),
# live capture-pane preview, Enter jumps, Esc = read-only peek.
bind S display-popup -E -w 90% -h 70% "~/.tmux/scripts/claude-fleet.sh"
```

### ~/.tmux/scripts/claude-fleet.sh

```bash
#!/usr/bin/env bash
# claude-fleet.sh — fzf picker over every Claude Code pane, with live preview.
# Invoked from the prefix+S display-popup in ../tmux.conf.
set -euo pipefail

rows() {
  local now; now=$(date +%s)
  # Beacon options (@claude_*) may be empty — Phase 1 falls back to the
  # pane-title heuristic, kept in sync with `talk ping` (~/.local/bin/talk).
  tmux list-panes -a -F \
    $'#{pane_id}\t#{session_name}:#{window_index}.#{pane_index}\t#{pane_title}\t#{b:pane_current_path}\t#{@claude_state}\t#{@claude_since}' |
  while IFS=$'\t' read -r id target title dir state since; do
    if [[ -z $state ]]; then
      if   [[ $title == "✳ "* || $title == "Claude Code" ]]; then state=idle
      elif [[ $title =~ ^[⠀-⣿] ]]; then state=busy      # braille spinner, needs UTF-8 locale
      else continue                                     # not a Claude pane
      fi
    fi
    local glyph prio elapsed=""
    case $state in
      idle)    glyph="●" prio=0 ;;
      blocked) glyph="✋" prio=0 ;;
      busy)    glyph="◐" prio=1 ;;
      *)       glyph="○" prio=2 ;;
    esac
    [[ -n $since ]] && elapsed=$(( (now - since) / 60 ))m || elapsed="—"
    local last
    last=$(tmux capture-pane -p -t "$id" -S -5 | awk 'NF {l=$0} END {print l}')
    printf '%s\t%d\t%s %-7s %5s  %-14s %-20s %.40s │ %.60s\n' \
      "$id" "$prio" "$glyph" "$state" "$elapsed" "$target" "$dir" "$title" "$last"
  done
}

pick=$(rows | sort -t$'\t' -k2,2n | fzf \
  --ansi --delimiter=$'\t' --with-nth=3 --no-sort \
  --header='Enter: jump   Esc: close (peek only)   ctrl-r: refresh preview' \
  --preview 'tmux capture-pane -e -p -t {1} -S -200' \
  --preview-window='right,60%' \
  --bind 'ctrl-r:refresh-preview') || exit 0

pane=${pick%%$'\t'*}
tmux select-window -t "$pane"
tmux select-pane   -t "$pane"
tmux switch-client -t "$pane"
```

Notes on the sketch:

- `{1}` in `--preview` is the hidden pane-id field; `--with-nth=3` hides both
  `pane_id` and the numeric sort key from display.
- `switch-client -t <pane_id>` resolves the pane's session; running tmux commands
  from inside a popup targets the invoking client — same mechanism the popup
  itself relies on.
- `sort` + `--no-sort` pins the attention-first ordering; fzf filtering still works.

### Phase 2: beacon hook (new file in the `.claude` submodule)

`~/.claude/hooks/claude-beacon.sh`:

```bash
#!/usr/bin/env bash
# claude-beacon.sh <state> — stamp this Claude pane's state into tmux pane options
# for the fleet popup (~/.tmux/scripts/claude-fleet.sh). No-op outside tmux.
[[ -n ${TMUX_PANE:-} && -n ${TMUX:-} ]] || exit 0
tmux set-option -p -t "$TMUX_PANE" @claude_state "${1:-busy}"
tmux set-option -p -t "$TMUX_PANE" @claude_since "$(date +%s)"
exit 0
```

Wired in [settings.json](../../.claude/settings.json) (alongside the existing
hook entries): `UserPromptSubmit` → `claude-beacon.sh busy`, `Stop` →
`claude-beacon.sh idle`, `Notification` → `claude-beacon.sh blocked`.
`@claude_activity` (current tool) would need a `PreToolUse` entry parsing
`tool_name` from stdin JSON — defer; the pane title already carries activity.
[未驗證] whether a `SessionEnd` event is available to *clear* the options when
Claude exits; if not, the fleet script should cross-check `pane_current_command`
before trusting a stale `@claude_state`.

## Integration with existing setup

- **[tmux.conf](../tmux.conf)** — one new `bind S`, same `display-popup -E` idiom
  as prefix+P and prefix+?. Place it next to those binds.
- **[cheat.txt](../cheat.txt)** — add one line under OTHER:
  `prefix S        Claude fleet: pick/peek any Claude pane (all sessions)`.
- **talk** (`~/.local/bin/talk`, untracked) — the Phase 1 state heuristic is
  copied from `cmd_ping`; both files get a cross-reference comment so a Claude
  Code title-format change is fixed in both places. Fleet replaces the "talk list
  + squint at titles" discovery flow but not talk's messaging.
- **`.claude` submodule** — Phase 2 files live there: commit inside
  `~/.claude` first, then bless the gitlink SHA in the parent (per
  [CLAUDE.md](../../CLAUDE.md) submodule conventions). The fleet script itself
  lives in `.tmux/` (parent repo) and must not hard-depend on the beacons.
- **ace-window** — complementary, not overlapping: prefix+o is "jump within what
  I can see", prefix+S is "find/peek across everything". No key conflicts.
- **tmux-thumbs** — not usable inside the popup's preview text; after jumping,
  prefix+Space works as usual on the real pane.
- **mq / monitor-tmux-pane skills** — untouched; the popup is human-facing.
  (Future: an mq `fleet` topic could feed richer activity strings — out of scope.)
- **`.gitignore`** — `.tmux/` whitelist must gain the new path: add `!/.tmux/scripts/`
  and `!/.tmux/scripts/**` (each level opened explicitly, per the repo's nested
  whitelist rule) plus `!/.tmux/plans/` if plans should be tracked. [未驗證]
  which exact `.tmux` rules exist today — check `.gitignore` before committing.

## Risks & open questions

- **Title heuristic fragility**: Phase 1 breaks if Claude Code changes its title
  format, if `set-titles`/apps overwrite pane titles, or under a non-UTF-8 locale
  (braille range match). Mitigation: Phase 2 beacons become the primary signal.
- **Stale beacons**: a pane whose Claude exited keeps `@claude_state` until the
  pane dies. Cross-check `pane_current_command` (should be `claude`/`node`
  [未驗證 which]) or clear on session end if the hook event exists.
- **Preview is a snapshot, not a stream**: fzf re-runs the preview on cursor move
  and on ctrl-r, but it does not tick while you sit on one row. A
  `--preview-window follow` + looping capture variant exists but re-appends full
  captures; keep snapshot + ctrl-r for MVP.
- **Popup key eating**: inside `display-popup`, the tmux prefix still works
  (popup grabs input otherwise); Esc-to-close relies on fzf's own abort binding —
  verify `escape-time 10` doesn't make Esc feel laggy in fzf.
- **Hook latency**: Phase 2 adds a subprocess on every UserPromptSubmit/Stop/
  Notification. `tmux set-option` is ~ms; acceptable, but keep the beacon script
  fork-free beyond the two tmux calls.
- **Zoomed panes**: `select-pane` on a zoomed window's hidden pane — decide
  whether to `resize-pane -Z` off first. [未驗證] current tmux behavior.

## MVP steps

1. **Script, standalone**: write `~/.tmux/scripts/claude-fleet.sh` (Phase 1
   heuristic only, no elapsed column). Test from any shell inside tmux by running
   it directly — verify only Claude panes list, preview renders with colors,
   Enter jumps cross-session, Esc is side-effect free.
2. **Bind + docs**: add `bind S` to [tmux.conf](../tmux.conf), reload
   (`tmux source ~/.tmux/tmux.conf`), add the [cheat.txt](../cheat.txt) line.
   Test `prefix+S` from two different sessions.
3. **Ordering + last-line column**: add the priority sort and `capture-pane`
   last-line suffix. Test with one busy + one idle Claude: idle sorts first,
   last line matches what the pane shows.
4. **Gitignore + commit**: open the `.gitignore` whitelist for
   `.tmux/scripts/`, commit script + bind + cheat line as one system-scoped
   commit (per repo commit conventions). No push.
5. **Beacon hooks (Phase 2)**: add `claude-beacon.sh` + three `settings.json`
   entries in the `.claude` submodule; verify with
   `tmux display -p -t <pane> '#{@claude_state}'` after prompting/stopping a
   test Claude. Commit inside submodule, bless SHA in parent.
6. **Fleet consumes beacons**: prefer `@claude_state`/`@claude_since` when
   non-empty (already coded in the sketch); verify the elapsed column ticks and
   `blocked` appears during a permission prompt.

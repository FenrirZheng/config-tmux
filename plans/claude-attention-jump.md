# Attention jump: bell on blocked, one key to it, one key back

Status: plan (2026-08-06). Repo: `~/.tmux` on branch `main`.

## Problem

In multi-agent runs (3–6 Claude Code instances across tmux windows/sessions), a
permission prompt in a *hidden* pane is the highest-cost stall: the agent sits
blocked, the whole dispatch it anchors goes dead, and nothing on screen says so.
Today the failure loop is: notice much later that "it's been quiet", cycle
`prefix n` / `prefix w` through every window scanning for a permission dialog,
answer it, then reconstruct where you were working before the hunt. Two costs,
both pure friction: the **visual search** ("which one pinged?") and the
**return trip** ("where was I?"). Claude Code already emits a `Notification`
hook event at exactly the right moment — nothing currently routes it into tmux.

## Design

### User-visible behavior

1. A hidden Claude hits a permission prompt (or goes idle waiting for input) →
   its window instantly shows tmux's native `!` bell flag in the status bar.
   No flag ever appears for the pane you are currently looking at.
2. `prefix N` → jump straight to the pane that most needs attention: the
   **oldest** `needs-input` pane (longest-blocked first); if none is blocked,
   the **newest** `idle` pane (most recently finished turn). Your origin pane
   is marked (`select-pane -m`) before the jump.
3. Answer the prompt. `prefix B` → bounce back to the marked origin. The mark
   is swapped on each bounce, so `B` toggles between the two panes.
4. The state self-drains: submitting a prompt to a pane (keyboard or `talk`
   injection) clears its flag; a finished turn downgrades it to `idle`.

### Components

- **Beacon (state substrate)** — new hook script `~/.claude/hooks/pane-state.sh`
  (lives in the `.claude` submodule), wired at three Claude Code hook events.
  It mirrors the agent lifecycle into per-pane tmux user options:
  `@claude_state` ∈ `needs-input` | `idle` | `busy`, plus `@claude_since`
  (epoch seconds). The event name is passed as `$1` from `settings.json`, so
  no stdin-JSON parsing is needed.
- **Bell** — the Notification branch checks
  `#{&&:#{window_active},#{pane_active}}` and `#{session_attached}`; if the
  pane is not on-screen it writes BEL to `#{pane_tty}`. tmux reads the pty
  master, and with `monitor-bell on` / `visual-bell off` (both already the
  live values on this server, tmux 3.5a — pinned in conf anyway) that sets the
  standard `!` window flag.
- **Navigation** — two ~30-line scripts in this repo,
  [claude-attend.sh](../scripts/claude-attend.sh) and
  [claude-return.sh](../scripts/claude-return.sh), bound to `prefix N` /
  `prefix B` (both unbound in stock tmux; verify with `tmux list-keys -T
  prefix | grep -E "'[NB]'"` before binding). They receive `#{client_name}`
  and `#{pane_id}` expanded at bind time, so cross-**session** jumps work via
  `switch-client -c`.

## Implementation sketch

### [tmux.conf](../tmux.conf) additions (before the theme block)

```tmux
# Attention jump: hidden Claude hits a permission prompt -> pane-state.sh
# (Claude Code Notification hook) writes BEL to the pane tty -> native '!'
# window flag. prefix+N jumps to the neediest pane, prefix+B bounces back.
set -g monitor-bell on      # server default, pinned for durability
set -g visual-bell off
set -g bell-action any
bind N run-shell "~/.tmux/scripts/claude-attend.sh '#{client_name}' '#{pane_id}'"
bind B run-shell "~/.tmux/scripts/claude-return.sh '#{client_name}' '#{pane_id}'"
```

### `~/.claude/hooks/pane-state.sh` (in the `.claude` submodule)

```bash
#!/usr/bin/env bash
# pane-state.sh <notification|stop|prompt> -- mirror Claude Code lifecycle
# into tmux pane options (@claude_state, @claude_since). Consumed by
# ~/.tmux/scripts/claude-attend.sh (prefix+N). Event arrives as $1 from
# settings.json wiring, so no stdin JSON parsing needed.
set -u
event="${1:?usage: pane-state.sh notification|stop|prompt}"
[ -n "${TMUX_PANE:-}" ] || exit 0                      # not inside tmux
tmux display -p -t "$TMUX_PANE" '' >/dev/null 2>&1 || exit 0  # pane gone
now=$(date +%s)
case "$event" in
  notification)
    tmux set -p -t "$TMUX_PANE" @claude_state needs-input
    tmux set -p -t "$TMUX_PANE" @claude_since "$now"
    visible=$(tmux display -p -t "$TMUX_PANE" '#{&&:#{window_active},#{pane_active}}')
    attached=$(tmux display -p -t "$TMUX_PANE" '#{session_attached}')
    if [ "$visible" != 1 ] || [ "$attached" = 0 ]; then
      printf '\a' > "$(tmux display -p -t "$TMUX_PANE" '#{pane_tty}')" 2>/dev/null || true
    fi ;;
  stop)   tmux set -p -t "$TMUX_PANE" @claude_state idle
          tmux set -p -t "$TMUX_PANE" @claude_since "$now" ;;
  prompt) tmux set -p -t "$TMUX_PANE" @claude_state busy
          tmux set -p -t "$TMUX_PANE" @claude_since "$now" ;;
esac
exit 0
```

### `settings.json` hook wiring (merge, don't clobber)

Current `~/.claude/settings.json` has `UserPromptSubmit` (cross-pane-detect)
but **no** `Notification` or `Stop` entries — note the global CLAUDE.md hook
table lists `cross-pane-enforce.sh` at Stop, which is not in the
`settings.json` on disk; it may live in root-owned `settings.local.json`
[未驗證] — so append, never rewrite, the arrays:

```json
"Notification": [
  { "matcher": "", "hooks": [
    { "type": "command", "command": "~/.claude/hooks/pane-state.sh notification" } ] }
],
"Stop": [
  { "matcher": "", "hooks": [
    { "type": "command", "command": "~/.claude/hooks/pane-state.sh stop" } ] }
],
"UserPromptSubmit": [
  { "matcher": "", "hooks": [
    { "type": "command", "command": "~/.claude/hooks/cross-pane-detect.sh" },
    { "type": "command", "command": "~/.claude/hooks/pane-state.sh prompt" } ] }
]
```

### [scripts/claude-attend.sh](../scripts/claude-attend.sh) — `prefix N`

```bash
#!/usr/bin/env bash
# prefix+N: jump to the Claude pane that most needs attention.
# Priority: oldest needs-input (longest-blocked), else newest idle.
set -u
client="${1:-}" ; origin="${2:-}"
tab=$(printf '\t')
pick() { # $1=state  $2=sort flag on @claude_since (n=oldest, rn=newest)
  tmux list-panes -a -F "#{@claude_state}${tab}#{@claude_since}${tab}#{pane_id}" \
    | awk -F"$tab" -v s="$1" '$1 == s && $2 != ""' \
    | sort -t"$tab" -k2,2"$2" | head -1 | cut -f3
}
target=$(pick needs-input n)
[ -n "$target" ] || target=$(pick idle rn)
[ -n "$target" ] || { tmux display-message "attend: no Claude pane needs input"; exit 0; }
[ -n "$origin" ] && tmux select-pane -m -t "$origin"   # mark for prefix+B
sess=$(tmux display -p -t "$target" '#{session_name}')
win=$(tmux display -p -t "$target" '#{window_id}')
if [ -n "$client" ]; then tmux switch-client -c "$client" -t "$sess"
else tmux switch-client -t "$sess"; fi
tmux select-window -t "$win"
tmux select-pane -t "$target"
```

### [scripts/claude-return.sh](../scripts/claude-return.sh) — `prefix B`

```bash
#!/usr/bin/env bash
# prefix+B: bounce back to the marked origin; swaps the mark so B toggles.
set -u
client="${1:-}" ; here="${2:-}"
origin=$(tmux display -p -t '{marked}' '#{pane_id}' 2>/dev/null || true)
[ -n "$origin" ] || { tmux display-message "return: no origin marked"; exit 0; }
[ -n "$here" ] && tmux select-pane -m -t "$here"
sess=$(tmux display -p -t "$origin" '#{session_name}')
win=$(tmux display -p -t "$origin" '#{window_id}')
if [ -n "$client" ]; then tmux switch-client -c "$client" -t "$sess"
else tmux switch-client -t "$sess"; fi
tmux select-window -t "$win"
tmux select-pane -t "$origin"
```

### [cheat.txt](../cheat.txt) — two lines under OTHER

```
prefix N        jump to Claude pane needing input (marks origin)
prefix B        bounce back to origin (repeat to toggle)
```

## Integration with existing setup

- **`talk` CLI + cross-pane hooks**: `talk send` keystroke-injection lands as a
  prompt submit in the target pane → the `prompt` branch clears its
  `needs-input`/`idle` state automatically. A peer agent answering a blocked
  sibling via talk drains the beacon with zero extra wiring. `pane-state.sh`
  sits beside `talk-wrap.sh` / `cross-pane-detect.sh` in the same hooks dir.
- **`mq`**: untouched. mq's Monitor-based reader wakes the *session*, not the
  human; this feature covers the human-attention half. A later feature could
  publish `needs-input` transitions to an mq topic — out of scope here.
- **ace-window (`prefix o`/`O`)**: complementary, not overlapping — ace-window
  is "go to a pane I can see"; `prefix N` is "go to the pane I *can't* see".
  Same-window fine-positioning after `N` still uses `prefix o`.
- **tmux-thumbs (`prefix Space`)**: unaffected; `N`/`B` don't touch copy flow.
- **tokyo-night theme**: renders window status via its own formats — verify the
  `!` bell flag survives its `window-status-format` [未驗證: theme may drop
  `#{window_flags}`]. If hidden, add a bell-styled
  `window-status-bell-style` or append `#{?window_bell_flag,!,}` after the
  theme's `run-shell` line. `prefix N` works regardless — the flag is
  cosmetics, the pane options are the real substrate.
- **`monitor-tmux-pane` / `claude-session-dispatch` skills**: gain a free query
  API — `tmux list-panes -a -F '#{pane_id} #{@claude_state} #{@claude_since}'`
  tells a dispatcher which agents are blocked without capture-pane scraping.
- **Repos & commits**: scripts + conf + cheat.txt → this repo (`~/.tmux`, one
  commit, `tmux: attention jump ...`). `pane-state.sh` + `settings.json` →
  commit inside the `.claude` submodule first, then bless the gitlink in the
  `$HOME` parent repo (per its CLAUDE.md). No push, no PR.

## Risks & open questions

- **Notification granularity**: the hook fires for permission prompts *and*
  the 60-second idle notification; both are treated as `needs-input`. That is
  acceptable (both mean "human wanted"), but if idle noise annoys, filter by
  parsing the `message` field from stdin JSON (`jq -r .message`, jq is
  installed) — deferred until proven necessary.
- **Stale `idle` panes**: a pane whose Claude session ended stays `idle`
  forever and can win the `prefix N` fallback. Mitigations: (a) wire a
  `SessionEnd` branch that runs `tmux set -pu @claude_state` [未驗證:
  SessionEnd hook availability in current Claude Code]; (b) have `pick idle`
  skip entries older than e.g. 2h. Ship without, add if it bites.
- **Audible bell passthrough**: with `visual-bell off` + `bell-action any`,
  tmux also forwards BEL to the outer terminal — Alacritty may ding/flash.
  Arguably desired ("bell on blocked"); if not, `set -g visual-bell on` turns
  it into a status-line message instead.
- **`run-shell` client context**: `switch-client` inside `run-shell` needs the
  right client; passing `#{client_name}` expanded at bind time sidesteps the
  ambiguity. Multiple attached clients each get correct behavior since each
  keypress expands its own client name.
- **Popup panes**: Claude launched inside `prefix P` display-popup has a
  transient `TMUX_PANE`; the `tmux display -t` guard exits 0 once it vanishes.
- **`prefix N`/`B` conflicts**: unbound in stock tmux and unused in
  [cheat.txt](../cheat.txt); confirm no plugin claimed them
  (`tmux list-keys -T prefix`) before committing.

## MVP steps

1. **Beacon, Notification only.** Create `~/.claude/hooks/pane-state.sh`
   (`chmod +x`), wire only the `Notification` entry in `settings.json`.
   Test without waiting for a real prompt:
   `TMUX_PANE=%<hidden-pane> ~/.claude/hooks/pane-state.sh notification`
   → `!` flag appears on that window; repeat on the *active* pane → no bell.
   Then trigger a real permission prompt in a hidden Claude and confirm.
2. **Full state machine.** Add `Stop` + `UserPromptSubmit` wiring. Verify
   transitions with `tmux show -p -t %<pane> @claude_state` across one full
   prompt→busy→needs-input→answer→busy→stop→idle cycle.
3. **`prefix N`.** Add `scripts/claude-attend.sh` + the bind + bell option
   pins to [tmux.conf](../tmux.conf); `tmux source ~/.tmux/tmux.conf`. Test:
   two hidden panes with staged `@claude_since` values → `N` lands on the
   older; no candidates → status message; origin pane shows the mark.
4. **`prefix B`.** Add `scripts/claude-return.sh` + bind. Test: `N` then `B`
   returns to origin; `B` again toggles forward; `B` with no mark → message.
   Cross-session: put the target in a second session, confirm
   `switch-client` path.
5. **Cheat sheet.** Add the two lines to [cheat.txt](../cheat.txt); check the
   popup still fits (`prefix ?`, popup is 78x34).
6. **Theme flag check.** Confirm tokyo-night shows the `!` flag; if not,
   append the `window_bell_flag` format fix (see Integration).
7. **Commit.** This repo: one commit (conf + 2 scripts + cheat.txt).
   `.claude` submodule: commit hook + settings, then bless the gitlink SHA in
   the `$HOME` parent. Verify a fresh `tmux kill-server`-free reload works.

# Claude state beacon: hooks-to-borders visibility substrate

Status: plan (2026-08-06). Repo: `~/.tmux` (this repo) + `~/.claude` (submodule of the
home repo — hook + settings changes land there).

## Problem

With 3-6 Claude Code instances in sibling panes/windows, "who finished?" and "who is
stuck on a permission prompt?" is answered today by cycling `prefix n` / `prefix o`
through every pane and eyeballing each TUI. A Claude waiting on a permission prompt in
a background window burns wall-clock silently; a finished agent sits idle while you
babysit a busy one. `talk ping` (see the `talk` skill) can probe one pane, but it is a
per-target heuristic, not ambient visibility. The tokyo-night status bar shows git +
path — nothing about Claude lifecycle. This bites hardest exactly during multi-agent
sessions (pair, claude-session-dispatch, mq relays), i.e. the setup's main use case.

## Design

User-visible behavior first:

1. **Pane borders** (`pane-border-status top`): each Claude pane shows a colored glyph
   + current activity — `● Bash: go test ./...` — yellow `●` busy, red `● NEEDS INPUT`
   for a permission/input prompt, green `●` idle (turn ended). Non-Claude panes show
   the plain default border text.
2. **Window names**: on SessionStart the window is renamed `cc:<basename cwd>` (e.g.
   `cc:coinsasia-py`) and `automatic-rename` is turned off, so the status bar reads as
   a fleet roster instead of `zsh`/`node`.
3. **Window strip**: each window's entry in the status bar carries the worst state of
   its panes (needs-input > busy > idle) as the same glyph, appended to the existing
   tokyo-night format.
4. **Fleet segment** in `status-right`: a compact `2● 5●` list of window indexes whose
   worst state is `needs-input` — visible from *any* window, which is the actual
   "who needs me" answer.

Components:

- **One hook script** `~/.claude/hooks/pane-state.sh <event>` (in the `.claude`
  submodule, English, same style as [cross-pane-detect.sh](../../.claude/hooks/cross-pane-detect.sh)),
  wired in [settings.json](../../.claude/settings.json) at UserPromptSubmit (`busy`),
  PreToolUse `*` (`activity` — also re-stamps `busy`, which covers the
  "permission approved → running again" transition that has no UserPromptSubmit),
  Notification (`attn`), Stop (`idle`), SessionStart (`session-start`), SessionEnd
  (`clear`).
- **State store**: tmux per-pane user options `@claude_state`, `@claude_since`,
  `@claude_activity`, plus a hook-computed per-window aggregate `@claude_wstate`.
  Verified on this machine (tmux 3.5a): per-pane `@`-options set with
  `set-option -p` resolve in formats, and window `@`-options resolve inside a
  `#{W:}` status-line loop.
- **Render layer**: pure tmux format lines appended to [tmux.conf](../tmux.conf)
  *after* the TPM `run` line (so they override tokyo-night's `pane-border-status off`
  and window-status formats). No daemon, no `#()` polling; hooks call
  `tmux refresh-client -S` so the bar updates instantly instead of at
  `status-interval`.

## Implementation sketch

### 1. `~/.claude/hooks/pane-state.sh`

```bash
#!/usr/bin/env bash
# pane-state.sh <busy|idle|attn|activity|session-start|clear>
# Stamps this Claude pane's lifecycle state into tmux per-pane user options.
# Read by ~/.tmux/tmux.conf border/status formats. Always exits 0 (never blocks).
set -eu
[[ -n "${TMUX_PANE:-}" ]] && command -v tmux >/dev/null 2>&1 || exit 0
pane="$TMUX_PANE" event="${1:-}" input=$(cat)

aggregate() {  # worst state across this window's panes -> window option
  local worst="" s
  while IFS= read -r s; do
    case "$s" in
      needs-input) worst="needs-input"; break ;;
      busy)        worst="busy" ;;
      idle)        [[ -z "$worst" ]] && worst="idle" ;;
    esac
  done < <(tmux list-panes -t "$pane" -F '#{@claude_state}')
  tmux set-option -w -t "$pane" @claude_wstate "$worst"
}

stamp() {
  tmux set-option -p -t "$pane" @claude_state "$1"
  tmux set-option -p -t "$pane" @claude_since "$(date +%s)"
  aggregate; tmux refresh-client -S
}

case "$event" in
  busy) stamp busy ;;
  attn) stamp needs-input ;;
  idle) tmux set-option -p -t "$pane" @claude_activity ""; stamp idle ;;
  activity)
    act=$(jq -r '"\(.tool_name): \(.tool_input.command // .tool_input.file_path
      // .tool_input.pattern // .tool_input.skill // "")"' <<<"$input" \
      | tr -d '#{}%\n' | cut -c1-48)   # strip format metachars, truncate
    tmux set-option -p -t "$pane" @claude_activity "$act"
    stamp busy ;;
  session-start)
    cwd=$(jq -r '.cwd // empty' <<<"$input"); : "${cwd:=$PWD}"
    tmux rename-window -t "$pane" "cc:$(basename "$cwd")"
    tmux set-option -w -t "$pane" automatic-rename off
    stamp idle ;;
  clear)
    for o in @claude_state @claude_since @claude_activity; do
      tmux set-option -pu -t "$pane" "$o" 2>/dev/null || true
    done
    aggregate; tmux refresh-client -S ;;
esac
exit 0
```

### 2. `~/.claude/settings.json` additions (merge into existing `hooks` object)

```json
"UserPromptSubmit": [{ "matcher": "", "hooks": [
  { "type": "command", "command": "~/.claude/hooks/cross-pane-detect.sh" },
  { "type": "command", "command": "~/.claude/hooks/pane-state.sh busy" } ]}],
"PreToolUse": [
  { "matcher": "Bash", "hooks": [ /* existing talk-wrap/gotest/sudo guards unchanged */ ] },
  { "matcher": "*", "hooks": [
    { "type": "command", "command": "~/.claude/hooks/pane-state.sh activity" } ]}],
"Notification": [{ "matcher": "", "hooks": [
  { "type": "command", "command": "~/.claude/hooks/pane-state.sh attn" } ]}],
"Stop": [{ "matcher": "", "hooks": [
  { "type": "command", "command": "~/.claude/hooks/pane-state.sh idle" } ]}],
"SessionStart": [{ "matcher": "", "hooks": [
  { "type": "command", "command": "~/.claude/hooks/pane-state.sh session-start" } ]}],
"SessionEnd": [{ "matcher": "", "hooks": [
  { "type": "command", "command": "~/.claude/hooks/pane-state.sh clear" } ]}]
```

Note: settings.json currently has **no** Stop entry even though the global CLAUDE.md's
hook table lists `cross-pane-enforce.sh` at Stop — that wiring is absent today
(verified by reading the file). If it gets re-wired, both commands live in the same
Stop array; order is irrelevant (pane-state never blocks).

### 3. [tmux.conf](../tmux.conf) — append at end of file (after the TPM `run` line)

```tmux
# ── Claude state beacon ─ reads @claude_* options stamped by
#    ~/.claude/hooks/pane-state.sh; overrides tokyo-night's pane-border-status off.
set -g pane-border-status top
set -g pane-border-format ' #{pane_index}#{ace_window_label:} \
#{?#{==:#{@claude_state},needs-input},#[fg=red bold]● NEEDS INPUT#[default],\
#{?#{==:#{@claude_state},busy},#[fg=yellow]●#[default] #{@claude_activity},\
#{?#{==:#{@claude_state},idle},#[fg=green]●#[default],#{pane_current_command}}}} '

# Append worst-state glyph to tokyo-night's window-status formats (read-modify-write
# at conf load; must run after the theme has set its formats).
run-shell 'tmux set -g window-status-format "$(tmux show -gv window-status-format)#{?#{==:#{@claude_wstate},needs-input},#[fg=red]●,#{?#{==:#{@claude_wstate},busy},#[fg=yellow]●,#{?#{==:#{@claude_wstate},idle},#[fg=green]●,}}} "'
run-shell 'tmux set -g window-status-current-format "$(tmux show -gv window-status-current-format)#{?#{==:#{@claude_wstate},needs-input},#[fg=red]●,#{?#{==:#{@claude_wstate},busy},#[fg=yellow]●,#{?#{==:#{@claude_wstate},idle},#[fg=green]●,}}} "'

# Fleet segment: indexes of windows whose worst state is needs-input, from anywhere.
set -ga status-right '#[fg=red,bold]#{W:#{?#{==:#{@claude_wstate},needs-input},#I● ,}}'
```

(`#{ace_window_label:}` placeholder shown for intent only — drop it in v1; see Risks.)

## Integration with existing setup

- **tokyo-night theme**: the plugin sets `pane-border-status off` and its own
  window-status formats at TPM run time; all beacon lines sit after that in
  [tmux.conf](../tmux.conf) so they win. The window-status append uses
  read-modify-write so the theme's icons/colors are preserved, not replaced.
- **talk / communicate-with / pair / claude-session-dispatch**: peers get ground
  truth — `tmux show -pv -t %N @claude_state` replaces the capture-and-eyeball
  heuristic inside `talk ping`. A `talk send` keystroke-injected prompt triggers the
  receiver's UserPromptSubmit → its pane flips busy automatically; no protocol change.
- **mq / monitor-tmux-pane / watch-queue**: a session parked in a Monitor shows
  `busy` between events (the Monitor is a running tool call) — correct but worth
  knowing when reading the fleet strip. These skills can later *read* `@claude_state`
  to pick idle workers; this plan only writes the substrate.
- **tmux-ace-window** (`prefix o`/`O`): unaffected — its labels are drawn as overlays,
  not via `pane-border-format`. Later nicety: color ace labels by state (out of scope).
- **tmux-thumbs**: matches pane *content*, not border text; the extra border row is
  invisible to it.
- **[cheat.txt](../cheat.txt)**: add one legend line under OTHER, e.g.
  `● claude beacon   yellow=busy  red=needs input  green=done   (border + status bar)`.
- **Hooks**: `pane-state.sh busy` rides in the same UserPromptSubmit array as
  `cross-pane-detect.sh`; PreToolUse guards for Bash are untouched (separate matcher).
- **Commit choreography** (per home-repo conventions): commit `pane-state.sh` +
  `settings.json` inside `~/.claude` first, bless the gitlink in the home repo; commit
  `tmux.conf` + `cheat.txt` + this plan in `~/.tmux` (`tmux: add claude state beacon`).

## Risks & open questions

- **Notification hook coverage** [未驗證]: assumed to fire for both permission prompts
  and idle-waiting-for-input notifications, and that no Notification fires during
  normal streaming. Must be confirmed empirically in MVP step 2; if it under-fires,
  `needs-input` silently degrades to `busy` (annoying, not wrong).
- **Single-pane border row**: with `pane-border-status top` a one-pane window also
  gets a border line, costing one row everywhere — including non-Claude windows
  [未驗證 whether 3.5a suppresses it for single panes]. Fallback: gate it per-window
  from the hook (`set -w pane-border-status top` in `session-start`) instead of `-g`.
- **Multiple Claudes in one window**: last SessionStart wins the `cc:` rename;
  `@claude_wstate` aggregation handles state correctly. Acceptable.
- **Pane-id recycling / crashed sessions**: SessionEnd `clear` handles clean exits; a
  SIGKILL'd Claude leaves a stale glyph until the pane dies (pane options die with the
  pane). `@claude_since` exists so a later "stale if > N min" format tweak is possible.
- **Format injection**: tool args flow into a tmux format; `tr -d '#{}%'` strips the
  metacharacters. Keep it — a `Bash: echo #{pane_pid}` activity must render literally.
- **`window-status-format` read-modify-write**: runs once per conf load; reloading
  tmux.conf twice appends the glyph twice. Guard later with a `@beacon_applied` flag
  option, or accept "reload = restart theme first". Open question for step 5.
- **automatic-rename off** persists after Claude exits; window keeps the `cc:` name.
  Acceptable — SessionEnd could restore it, but the pane usually closes anyway.
- **Rejected alternative**: polling `#()` shell segments — slower (status-interval
  granularity), spawns processes per redraw, and the hook events already exist.

## MVP steps

1. **Script only.** Write `pane-state.sh` in `~/.claude/hooks/`, `chmod +x`, test by
   hand: `echo '{}' | TMUX_PANE=$TMUX_PANE ~/.claude/hooks/pane-state.sh busy` then
   `tmux show -pv @claude_state` → `busy`; repeat for `attn`/`idle`/`clear` and check
   `tmux show -wv @claude_wstate` aggregation with two panes.
2. **Wire hooks.** Merge the settings.json additions, start a fresh `claude`, run a
   long Bash tool, trigger a permission prompt, let a turn end — watch
   `@claude_state`/`@claude_activity` flip via `tmux show -pv` in a sibling pane.
   Verify the Notification assumption here.
3. **Pane borders.** Append `pane-border-status`/`pane-border-format` to tmux.conf,
   `tmux source ~/.tmux/tmux.conf`. Check glyph + activity render, non-Claude panes
   show `pane_current_command`, and the single-pane row cost is acceptable.
4. **Window rename.** Confirm `session-start` renames to `cc:<repo>` and
   `automatic-rename off` sticks; check tokyo-night still shows the name.
5. **Status bar.** Add the two `run-shell` window-status appends + the `status-right`
   fleet segment; verify glyphs for background windows and the double-append-on-reload
   behavior; decide on the guard flag.
6. **Docs + commit.** cheat.txt legend line, hook table row in the global CLAUDE.md's
   "Active hooks" section, then the two-repo commit choreography above.

Each step is independently revertable: step N broken → `tmux set -gu` the touched
options / drop the appended conf lines, earlier steps keep working.

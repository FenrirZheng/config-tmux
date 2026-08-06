# Layout resurrect: the whole agent team back after a reboot

Status: plan (2026-08-06). Repo: `~/.tmux` (standalone git repo; note the parent `$HOME`
repo ignores `.tmux` entirely — everything here commits to *this* repo, while the hook +
settings changes commit to the `~/.claude` repo).

## Problem

A multi-agent working set is expensive to assemble: a 4-pane pair/review layout means 4
`claude` sessions, each with its own cwd, its own conversation context, plus the talk/mq
wiring between them. A reboot, tmux server kill, or OOM destroys all of it at once.
Today recovery is fully manual: rebuild windows and splits by hand, `cd` each pane,
then hunt through `claude --resume`'s interactive picker four times trying to remember
which conversation belonged in which pane. That is ~15 minutes of reassembly, and the
spatial mapping ("reviewer was top-right") is usually lost even when the conversations
are found. tmux-resurrect is not installed and would not help with the hard part anyway:
it restores layouts, not *which Claude session UUID lived in which pane*.

The two proven primitives already exist in this stack:

- tmux layout strings (`#{window_layout}` + `select-layout`) round-trip exactly.
- `claude --resume <full-uuid>` restores a conversation with full memory — the
  `claude-session-dispatch` skill (`~/.claude/skills/claude-session-dispatch/`) documents
  that the **full** UUID is required (short 8-char ids are rejected) and that transcripts
  live under `~/.claude/projects/<slug>/<uuid>.jsonl`.

What is missing is the glue: nobody records which UUID is in which pane.

## Design

User-visible behavior first:

1. **Panes self-stamp.** Every time a `claude` session starts (or resumes, or `/clear`s)
   inside tmux, the pane silently gains `@claude_session_id`. On the first real prompt,
   the pane also gets a 3–4 word task slug as its border title (`fix-keyd-ime-toggle`),
   so a 4-pane team is visually labeled without any manual `select-pane -T`.
2. **`prefix + M-s` = snapshot.** One keystroke writes every session/window layout, every
   pane cwd, and every stamped UUID + task slug to `~/.tmux/state/last-layout.tsv`
   (plus a timestamped copy). A status-line message confirms: `snapshot: 3 windows,
   4 claude panes`.
3. **`claude-restore.sh` = resurrect.** After reboot, from any shell:
   `~/.tmux/scripts/claude-restore.sh`. It rebuilds the sessions/windows/splits, reapplies
   the exact layout strings, `cd`s every pane to its old cwd, and types
   `claude --resume <uuid>` + Enter into each formerly-Claude pane. The team comes back
   mid-conversation, in the same spatial arrangement. Non-Claude panes come back as
   plain shells in the right directory.

Components (three files + two config touches):

| piece | where | role |
|---|---|---|
| `pane-state.sh` | `~/.claude/hooks/` | one script, dispatches on `hook_event_name`: SessionStart stamps `@claude_session_id`; UserPromptSubmit sets slug title once; SessionEnd unsets the stamp |
| `claude-snapshot.sh` | [`scripts/`](../scripts/claude-snapshot.sh) | serialize server state to TSV |
| `claude-restore.sh` | [`scripts/`](../scripts/claude-restore.sh) | replay TSV into a live server |
| tmux.conf lines | [`tmux.conf`](../tmux.conf) | `bind M-s`, pane-border title rendering |
| settings.json entries | `~/.claude/settings.json` | wire pane-state.sh into 3 hook events |

Deliberately narrower than tmux-resurrect: no running-program detection, no scrollback
capture, no shell history — only layout, cwd, and Claude resumption.

## Implementation sketch

### 1. `~/.claude/hooks/pane-state.sh` (new, in the `~/.claude` repo)

```bash
#!/usr/bin/env bash
# Stamp tmux panes with Claude session state. Wired to SessionStart,
# UserPromptSubmit and SessionEnd in settings.json. No stdout on
# UserPromptSubmit (stdout would be injected into context).
set -eu
[[ -n "${TMUX_PANE:-}" ]] || exit 0            # not inside tmux: no-op
command -v jq >/dev/null || exit 0             # fail open, like cross-pane-detect.sh

input=$(cat)
event=$(jq -r '.hook_event_name // empty' <<<"$input")
sid=$(jq -r '.session_id // empty' <<<"$input")

case "$event" in
  SessionStart)                                # fires on startup/resume/clear alike
    tmux set-option -p -t "$TMUX_PANE" @claude_session_id "$sid" ;;
  SessionEnd)                                  # pane no longer resumable → unstamp
    tmux set-option -pu -t "$TMUX_PANE" @claude_session_id 2>/dev/null || true ;;
  UserPromptSubmit)
    # first prompt only, and never from relayed peer traffic
    [[ -n "$(tmux show -pqv -t "$TMUX_PANE" @claude_task)" ]] && exit 0
    prompt=$(jq -r '.prompt // empty' <<<"$input")
    grep -qE '^### \[talk\]|^-----mq ' <<<"$prompt" && exit 0
    slug=$(tr -cs '[:alnum:]' ' ' <<<"$prompt" | awk '{n=(NF<4?NF:4);
      for(i=1;i<=n;i++)printf "%s%s",tolower($i),(i<n?"-":"")}' | cut -c1-32)
    [[ -n "$slug" ]] || exit 0
    tmux set-option -p -t "$TMUX_PANE" @claude_task "$slug"
    tmux select-pane -t "$TMUX_PANE" -T "$slug" ;;
esac
exit 0
```

### 2. `~/.claude/settings.json` additions (existing hooks block, same repo)

Append to the existing `UserPromptSubmit` array (after `cross-pane-detect.sh`) and add
two new event keys:

```json
"SessionStart":  [{ "matcher": "", "hooks": [{ "type": "command", "command": "~/.claude/hooks/pane-state.sh" }] }],
"SessionEnd":    [{ "matcher": "", "hooks": [{ "type": "command", "command": "~/.claude/hooks/pane-state.sh" }] }],
"UserPromptSubmit": [ { "...existing cross-pane-detect entry..." : "" },
                      { "matcher": "", "hooks": [{ "type": "command", "command": "~/.claude/hooks/pane-state.sh" }] } ]
```

### 3. [`scripts/claude-snapshot.sh`](../scripts/claude-snapshot.sh)

```bash
#!/usr/bin/env bash
set -eu
out=~/.tmux/state/last-layout.tsv
mkdir -p ~/.tmux/state
: > "$out.tmp"
tmux list-sessions -F '#{session_name}' | while read -r s; do
  printf 'S\t%s\n' "$s" >> "$out.tmp"
  tmux list-windows -t "$s" -F '#{window_index}	#{window_name}	#{window_layout}' |
  while IFS=$'\t' read -r wi wn wl; do
    printf 'W\t%s\t%s\t%s\n' "$wi" "$wn" "$wl" >> "$out.tmp"
    tmux list-panes -t "$s:$wi" \
      -F 'P	#{pane_index}	#{pane_current_path}	#{@claude_session_id}	#{@claude_task}' >> "$out.tmp"
  done
done
mv "$out.tmp" "$out"
cp "$out" ~/.tmux/state/layout-$(date +%Y%m%d-%H%M).tsv   # keep history, prune by hand
tmux display-message "snapshot: $(grep -c '^W' "$out") windows, $(awk -F'\t' '$1=="P"&&$4!=""' "$out" | wc -l) claude panes"
```

### 4. [`scripts/claude-restore.sh`](../scripts/claude-restore.sh) (skeleton)

```bash
#!/usr/bin/env bash
set -eu
f=${1:-~/.tmux/state/last-layout.tsv}
while IFS=$'\t' read -r tag a b c d; do
  case "$tag" in
    S) sess=$a
       tmux has-session -t "=$sess" 2>/dev/null && sess="${a}-restored"
       tmux new-session -d -s "$sess" -x 220 -y 60; first_win=1 ;;
    W) win=$a; layout=$c; npanes=0
       if [[ -n "${first_win:-}" ]]; then
         tmux rename-window -t "$sess:^" "$b"; tmux move-window -s "$sess:^" -t "$sess:$win" 2>/dev/null || true
         first_win=
       else tmux new-window -d -t "$sess:$win" -n "$b"; fi ;;
    P) cwd=$b; uuid=$c; task=$d
       if (( npanes > 0 )); then tmux split-window -d -t "$sess:$win" -c "$cwd"
       else tmux send-keys -t "$sess:$win.0" -l " cd $(printf '%q' "$cwd")" && tmux send-keys -t "$sess:$win.0" Enter; fi
       npanes=$((npanes+1))
       target="$sess:$win.$((npanes-1))"
       [[ -n "$task" ]] && tmux select-pane -t "$target" -T "$task"
       if [[ -n "$uuid" ]]; then
         if ls ~/.claude/projects/*/"$uuid".jsonl >/dev/null 2>&1; then
           tmux send-keys -t "$target" -l "claude --resume $uuid"; tmux send-keys -t "$target" Enter
         else tmux display-message "restore: transcript missing for $uuid"; fi
       fi ;;
  esac
  # after the last pane of a window: reapply exact geometry
  [[ "$tag" == P ]] && tmux select-layout -t "$sess:$win" "$layout" 2>/dev/null || true
done < "$f"
```

(The real script buffers panes per window and applies `select-layout` once per window
after all splits exist — the skeleton shows the mechanism, the MVP steps harden it.)

### 5. [`tmux.conf`](../tmux.conf) additions

```tmux
# Claude layout snapshot (restore: ~/.tmux/scripts/claude-restore.sh after reboot)
bind M-s run-shell "~/.tmux/scripts/claude-snapshot.sh"

# Render the task slug stamped by ~/.claude/hooks/pane-state.sh.
# pane-border-status is currently off; only turn on when >1 pane would benefit —
# single-pane windows keep a clean border-less look via the window override below.
set -g pane-border-status top
set -g pane-border-format '#[bold]#{?#{@claude_task}, #{@claude_task} , #{pane_title} }'
set-hook -g window-layout-changed 'set -wF pane-border-status "#{?#{==:#{window_panes},1},off,top}"'
```

## Integration with existing setup

- **`claude-session-dispatch` skill** — the authority for the resume path this plan leans
  on: full-UUID-only `--resume`, transcript location, "transcript appears late" caveat.
  Restore's transcript-existence check reuses its documented layout.
- **hooks** — `pane-state.sh` joins the existing family in `~/.claude/hooks/`; it copies
  `cross-pane-detect.sh`'s conventions (jq fail-open, `set -eu`, stdin JSON parse). The
  UserPromptSubmit slug branch explicitly skips both peer-traffic banners (`### [talk]`
  from talk, `-----mq ` from mq) so a relayed message never becomes a pane title.
- **talk / mq** — no protocol changes. Bonus: slug titles make `talk list` pane picking
  easier because the border already says what each agent is doing.
- **ace-window** ([`plugins/tmux-ace-window`](../plugins/tmux-ace-window/README.md)) —
  its labels overlay pane borders; with `pane-border-status top` on, verify the letter
  overlay still lands visibly (its `@ace-window-label-bg 'default'` styling was tuned
  for border blending). If they collide, scope border-status per-window instead.
- **thumbs** — untouched; the restore script's typed `claude --resume <uuid>` lines are
  even thumbs-copyable from scrollback as a manual fallback.
- **[cheat.txt](../cheat.txt)** — add one line under OTHER:
  `prefix M-s   snapshot claude layout (restore: ~/.tmux/scripts/claude-restore.sh)`.
  Note cheat.txt is currently untracked in this repo (`?? cheat.txt`) — track it in the
  same commit.
- **"beacon" pane-border feature** — the mechanism sketch assumed an existing
  pane-border-format from a beacon feature; **verified absent** (no `pane-state.sh`,
  `pane-border-status` is `off` server-wide). This plan therefore ships its own minimal
  border wiring (§5); if a beacon feature lands later, merge its format string with
  `@claude_task` rather than duplicating `pane-border-format`.

## Risks & open questions

- **Resume may fork the session id.** [未驗證] whether current `claude --resume` keeps
  the old UUID or continues under a new one. Either way self-heals: SessionStart fires
  on resume and re-stamps whatever id the new process reports, so the *next* snapshot is
  correct. Verify once in MVP step 2.
- **Stale stamps.** SessionEnd doesn't fire on SIGKILL/reboot — a pane can carry a UUID
  whose process is gone. Harmless for this feature (that pane is exactly what we want to
  resume), but snapshot trusts the stamp blindly; the transcript check in restore is the
  backstop.
- **`send-keys` into a resumed picker/prompt.** If `claude --resume <uuid>` errors
  (deleted transcript, version change), the pane is left with an error message, not a
  broken layout — acceptable. Do not auto-retry.
- **Restore terminal size.** Layout strings encode absolute cell sizes; applying them to
  a differently-sized terminal makes tmux rescale proportionally — fine, but `new-session
  -x -y` should match the real client. Open question: read client size instead of the
  hardcoded 220x60.
- **Trigger timing.** Snapshot is manual (`prefix M-s`). Auto-snapshot via
  `set-hook -g client-detached` or a cron is tempting but risks snapshotting a
  half-torn-down server. Decide after real usage.
- **Concurrent Claudes in one pane** (popup via `prefix P` running claude): the popup's
  hook sees its own `TMUX_PANE` (the popup pane), which dies with the popup — stamp is
  lost with the pane, no corruption. Acceptable.
- **State file secrecy.** UUIDs + cwds only, no tokens. Keep `state/` out of this repo's
  [.gitignore](../.gitignore) whitelist anyway (add `state/` to it) — machine state, and
  the timestamped copies grow unbounded until pruned.

## MVP steps

1. **Stamping** — write `pane-state.sh` (SessionStart branch only), wire SessionStart in
   `settings.json`. Test: start `claude` in a pane, `tmux show -pv @claude_session_id`
   shows a UUID matching a file in `~/.claude/projects/`.
2. **Resume-id behavior probe** — `claude --resume <that-uuid>`, re-check the stamp;
   record whether the id was kept or forked (closes the [未驗證] above).
3. **Slug titles** — add UserPromptSubmit + SessionEnd branches, add §5 border lines to
   [tmux.conf](../tmux.conf). Test: first prompt titles the pane; `### [talk]` messages
   don't; `/clear` + new prompt keeps the old title (by design — re-title manually).
4. **Snapshot** — `claude-snapshot.sh` + `bind M-s`. Test: TSV lines match
   `list-windows`/`list-panes` output; display-message counts are right.
5. **Restore, layout only** — `claude-restore.sh` without the resume branch, run against
   a snapshot of a 2-window/4-pane session into a *second* tmux server
   (`tmux -L test`). Test: layouts and cwds identical.
6. **Restore, full** — enable the resume branch; kill the real server, restore, confirm
   all 4 Claudes come back mid-conversation. Time it (target: <60 s vs ~15 min manual).
7. **Docs + commits** — cheat.txt line; commit `.tmux` repo (scripts, conf, cheat.txt,
   gitignore `state/`) and `~/.claude` repo (hook, settings.json) as separate
   system-scoped commits per house convention.

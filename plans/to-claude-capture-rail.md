# to-Claude capture rail: marked target pane + three senders

Status: plan, 2026-08-06. Repo: `~/.tmux` (its own git repo, submodule of the home dotfiles repo).

## Problem

The single most frequent cross-pane act in this setup — ferrying a `file:line`, a stack
trace, or a build failure from one pane into a Claude Code composer in a sibling pane —
currently costs five manual steps every time: enter copy-mode, select, copy, switch pane,
paste, then often fix quoting or re-type the path as an `@`-reference. `talk` solves
agent→agent messaging (and submits with Enter), but there is no *human-driven*,
one-keystroke path that drops evidence into a Claude composer **unsubmitted**, leaving
the cursor for the actual question. It bites hardest during debugging loops: a test fails
in pane A, the question goes to Claude in pane B, ten times an hour.

## Design

### User-visible behavior

1. **Mark the inbox.** `prefix + m` (stock tmux mark) on the Claude pane. Its border
   shows a `▶ CLAUDE` badge so the current target is always visible. `prefix + M`
   (stock) clears it.
2. **Sender 1 — thumbs UPPERCASE hint (`file:line` → `@`-ref).** `prefix + Space`, then
   press the hint letter in UPPERCASE: instead of copying to the clipboard (lowercase,
   unchanged), the match is typed into the marked Claude composer as `@foo.vue:181 `,
   unsubmitted. The existing `@thumbs-regexp-1` already matches `foo.vue:181`.
3. **Sender 2 — copy-mode `Y` (selection → one paste).** Select any region in copy-mode,
   press `Y`: the selection lands in the composer as **one** bracketed paste (a single
   `[Pasted text]` unit, however many lines), unsubmitted. Clipboard untouched.
4. **Sender 3 — `prefix + e` (failing pane → `@`-file).** From the failing pane:
   captures its tail (last 200 lines) to `/tmp/claude-capture/<ts>-<pane>.txt` and types
   `[capture from <cwd>] @/tmp/claude-capture/<ts>-<pane>.txt ` into the target.
   Special case: if the *current* pane is the marked one (mark-on-source), capture the
   full joined scrollback (`-S -10000 -J`) instead and resolve the target by fallback.
   The `@`-file path keeps huge captures out of pasted context — Claude reads the file.

Nothing ever presses Enter; every sender leaves the composer focused on the user's
next words.

### Components

- **`bin/to-claude`** — the one shared resolver + deliverer. Resolves the target
  (`{marked}` first; fallback: most recently active pane running `claude`), reads stdin,
  delivers via one of three modes: `--ref` (one-liner → `@X ` via `send-keys -l`),
  `--type` (literal text via `send-keys -l`), `--paste` (named buffer +
  `paste-buffer -d -r -p`, the exact delivery pattern already proven in
  `~/.local/bin/talk`'s `send_text`). Prints the resolved target with
  `tmux display-message` so silent mis-targeting is visible.
- **`bin/fail-to-claude`** — thin wrapper for sender 3: `capture-pane` → temp file →
  `to-claude --type`.
- **[tmux.conf](../tmux.conf) wiring** — one thumbs option, two copy-mode binds, one
  prefix bind, and a border-badge override that must run *after* the tokyo-night theme.
- **[cheat.txt](../cheat.txt) section** — a `TO CLAUDE` block (4 lines).

## Implementation sketch

### `~/.tmux/bin/to-claude`

```bash
#!/usr/bin/env bash
# to-claude — deliver stdin into the marked Claude pane's composer, unsubmitted.
# Modes: --ref (one-liner -> "@X "), --type (literal), --paste (bracketed paste).
set -euo pipefail
mode=${1:---paste}

resolve_target() {
  if [[ $(tmux display-message -p '#{pane_marked_set}') == 1 ]]; then
    tmux display-message -p -t '{marked}' '#{pane_id}'
  else
    # Fallback: most recently active pane whose command is claude.
    tmux list-panes -a -F '#{t:pane_last_activity} #{pane_id} #{pane_current_command}' \
      | awk '$NF == "claude" {print}' | sort -rn | awk 'NR==1 {print $(NF-1)}'
  fi
}

target=$(resolve_target)
[[ -n $target ]] || { tmux display-message 'to-claude: no marked pane, no claude pane'; exit 1; }

case $mode in
  --ref)
    ref=$(head -c 4096 | tr -d '\n')
    tmux send-keys -t "$target" -l -- "@${ref} " ;;
  --type)
    text=$(head -c 16384)
    tmux send-keys -t "$target" -l -- "$text" ;;
  --paste)
    buf="toclaude-$$-$RANDOM"
    tmux load-buffer -b "$buf" -
    tmux paste-buffer -d -r -p -b "$buf" -t "$target" ;;  # -p: one bracketed-paste unit
  *) echo "usage: to-claude --ref|--type|--paste" >&2; exit 2 ;;
esac
tmux display-message "→ CLAUDE ${target}"
```

### `~/.tmux/bin/fail-to-claude`

```bash
#!/usr/bin/env bash
# fail-to-claude <pane_id> — capture a failing pane, type an @-file ref to Claude.
set -euo pipefail
src=${1:?pane_id}
dir=/tmp/claude-capture; mkdir -p "$dir"
out="$dir/$(date +%Y%m%d-%H%M%S)-${src#%}.txt"
cwd=$(tmux display-message -p -t "$src" '#{pane_current_path}')
marked=$(tmux display-message -p -t "$src" '#{pane_marked}')
if [[ $marked == 1 ]]; then                 # mark-on-source: full joined scrollback
  tmux capture-pane -p -J -S -10000 -t "$src" > "$out"
else
  tmux capture-pane -p -J -S -200 -t "$src" > "$out"
fi
printf '[capture from %s] @%s ' "$cwd" "$out" | ~/.tmux/bin/to-claude --type
```

### [tmux.conf](../tmux.conf) additions

```tmux
# --- to-Claude capture rail -------------------------------------------------
# Thumbs UPPERCASE hint: send match as @-ref to the marked Claude pane.
# (lowercase keeps the wl-copy behavior above; must precede the thumbs run-shell)
set -g @thumbs-upcase-command 'echo -n {} | ~/.tmux/bin/to-claude --ref'

# Copy-mode Y: pipe selection to the marked Claude pane as one bracketed paste.
# mode-keys here is emacs (verified 2026-08-06); bind both tables for safety.
bind -T copy-mode    Y send-keys -X copy-pipe-and-cancel '~/.tmux/bin/to-claude --paste'
bind -T copy-mode-vi Y send-keys -X copy-pipe-and-cancel '~/.tmux/bin/to-claude --paste'

# prefix + e: capture this pane's failure tail -> @-file ref in the Claude pane.
# (e is unbound in stock tmux; E = spread-layout is untouched)
bind e run-shell '~/.tmux/bin/fail-to-claude #{pane_id}'

# Border badge for the marked pane. tokyo-night.tmux runs `set -g
# pane-border-status off` when TPM loads it, and run-shell commands execute
# after config parsing — so this MUST be a run-shell placed after the theme
# lines (queued later => wins). Keep it the LAST line of this file.
run-shell 'tmux set -g pane-border-status top ; tmux set -g pane-border-format "#{?pane_marked,#[bold]#[fg=green] ▶ CLAUDE #[default],}#{?pane_active,#[reverse],}#{pane_index}#[default] \"#{pane_title}\""'
```

The `@thumbs-upcase-command` line goes next to the existing `@thumbs-command`
(before the `run-shell ~/.tmux/plugins/tmux-thumbs/tmux-thumbs.tmux` line — thumbs
reads its params at load). `upcase-command` is a real thumbs param
(`add-param upcase-command string`, tmux-thumbs.sh:49 — verified).

### [cheat.txt](../cheat.txt) addition (after COPY & GRAB)

```
  TO CLAUDE    prefix m        mark Claude pane as inbox (border shows ▶ CLAUDE)
               thumbs SHIFT+ltr  send that file:line as @-ref into Claude
               copy-mode Y     send selection into Claude as one paste
               prefix e        capture this pane's failure tail → @-file to Claude
```

## Integration with existing setup

- **tmux-thumbs** — composes via the documented `upcase-command` hook; the existing
  lowercase `@thumbs-command` (wl-copy) and `@thumbs-regexp-1` (`file:line`) are
  untouched, so clipboard workflows stay clean.
- **talk** — `to-claude --paste` reuses talk's proven `set-buffer`/`paste-buffer -d -r -p`
  delivery. Division of labor stays sharp: talk = agent→agent, submits with Enter,
  wrapped by `talk-wrap.sh`; to-claude = human→agent, never submits, runs from tmux
  key tables so no Claude hooks (`talk-wrap`, `cross-pane-*`) are ever in the path.
- **mq** — untouched; mq is session↔session pub/sub, this rail is keystroke-level.
- **tmux-ace-window** — complementary: `prefix + o` jumps *to* the marked pane after a
  send; no key conflicts (`o`/`O` taken by ace, `m`/`M` stock mark, `e` was unbound,
  `Y` unbound in the emacs copy-mode table).
- **tokyo-night theme** — the only collision: it forces `pane-border-status off`
  (tokyo-night.tmux:27). Handled by the ordered `run-shell` override above.
- **cheat.txt** — gains the `TO CLAUDE` block; it is currently untracked in this repo
  (`?? cheat.txt`) — add it in the same commit.
- **`.claude` hooks (phase 2, optional)** — a SessionStart/Stop hook in the `.claude`
  submodule could run `tmux set -pt "$TMUX_PANE" @claude_state active|idle`, letting the
  resolver prefer *idle* Claude panes. Nothing sets `@claude_state` today (verified by
  rg across `~/.claude/hooks/` and `~/.local/bin/`) — MVP uses `pane_current_command`.

## Risks & open questions

- **`pane_current_command` may not be `claude`.** If Claude Code shows as `node`, the
  fallback resolver finds nothing [未驗證 — check with `talk list` while a session runs].
  Mitigation: match `claude|node` plus `pane_title`, or ship phase-2 `@claude_state`.
- **Bracketed paste into the composer.** `paste-buffer -p` only brackets if the app
  enabled bracketed-paste mode; talk already relies on this against Claude Code, but the
  "multi-line arrives as one `[Pasted text]` unit, no submit" behavior should be the
  first thing MVP step 1 verifies [未驗證].
- **`@`-ref path relativity.** A thumbs-matched `src/foo.vue:181` is relative to the
  *source* pane's cwd; if the Claude pane's cwd differs the `@`-ref may not resolve.
  Open question: should `--ref` absolutize relative paths against the source pane's
  `pane_current_path` (needs the source pane id passed through the thumbs command)?
- **run-shell ordering.** The badge override depends on run-shell queue order (ours after
  TPM's theme load). If a future theme update changes behavior, symptom = no border rows;
  fix = re-check with `tmux show -g pane-border-status`.
- **Border rows cost one line per pane row.** If that grates, refinement: rebind
  `prefix m` / `M` to toggle `pane-border-status` window-locally alongside the mark.
- **Mark death.** Killing the marked pane silently flips resolution to the fallback;
  the `display-message "→ CLAUDE %N"` after every send is the guard — watch it.
- **Multiple Claude panes.** Fallback picks the most recently active one; ambiguity is
  by-design resolved by *marking explicitly* — the badge makes the contract visible.

## MVP steps (each independently testable)

1. **`bin/to-claude`** (`chmod +x`). Test without any wiring: mark a Claude pane, then
   `echo -n foo.vue:181 | ~/.tmux/bin/to-claude --ref` and
   `printf 'a\nb\nc\n' | ~/.tmux/bin/to-claude --paste` from another pane. Verify:
   `@`-ref typed unsubmitted; 3-liner arrives as one `[Pasted text]`; no Enter fired.
2. **Border badge.** Append the `run-shell` override to [tmux.conf](../tmux.conf),
   `tmux source-file ~/.tmux/tmux.conf`, mark/unmark a pane, verify `▶ CLAUDE` appears
   and survives a fresh server start (theme-ordering check).
3. **Thumbs sender.** Add `@thumbs-upcase-command`, reload, `prefix + Space` on a pane
   showing `foo.vue:181`, press the hint UPPERCASE → `@foo.vue:181 ` in the composer;
   lowercase still lands in wl-copy.
4. **Copy-mode `Y`.** Add both table binds, reload; select a multi-line stack trace,
   `Y`, verify single-paste delivery and copy-mode exit.
5. **`bin/fail-to-claude` + `prefix e`.** Test tail mode from an unmarked pane, then
   mark-on-source full-scrollback mode; verify the capture file exists and the typed
   line reads `[capture from <cwd>] @/tmp/claude-capture/... `.
6. **cheat.txt block** + start tracking the file.
7. Commit in `~/.tmux` (one system-scoped commit: scripts + conf + cheat), then bless
   the new SHA in the parent home repo per its submodule convention.
8. *(Phase 2, optional)* `@claude_state` SessionStart/Stop hook in the `.claude`
   submodule + resolver preference for idle panes.

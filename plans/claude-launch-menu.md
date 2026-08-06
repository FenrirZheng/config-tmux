# Claude launch menu: every way a Claude session starts, one keystroke

Status: plan (2026-08-06). Repo: `~/.tmux` (submodule `config-tmux.git` of the `$HOME` dotfiles repo).

## Problem

Starting a Claude Code session today is a ritual repeated many times a day, with four
distinct frictions:

1. **cd-and-type.** Split a pane (`prefix %`), wait for the shell, `cd` (the split via
   plain `split-window` doesn't inherit cwd), type `claude`. Three steps for the single
   most common action in this setup.
2. **`--continue` only resumes the latest.** Any older session in the same project needs
   `claude --resume <uuid>`, and the UUIDs live as opaque filenames under
   `~/.claude/projects/<slug>/*.jsonl` — nobody remembers them, so older sessions are
   effectively lost without spelunking.
3. **Duplicate-session sprawl.** "Work on project X" usually means a *new* window running
   claude in X — even when a window already sits on that project two windows away.
   Nothing jumps to the existing one.
4. **The `/pair` choreography is mirrored and error-prone.** Pairing two Claudes means:
   split, launch claude, wait for it to be ready, then type `/pair A B` in one pane and
   `/pair B A` (args swapped) in the other — see [pair.md](../../.claude/commands/pair.md).
   Getting the mirror wrong silently breaks the mq handshake.

Four raw ideas as four keybindings would violate the cheat-sheet's own footer policy
("one prefix per layer; adding namespaces is the disease"). One `display-menu` is the
compositional glue: one keystroke to learn, entries self-document.

## Design

### User-visible behavior

`prefix + C` pops a centered tmux menu titled **Claude**:

| key | entry | effect |
|---|---|---|
| `s` | Split right | new right pane in this pane's cwd, running `claude` |
| `w` | New window | new window in this cwd, named `cc:<dirname>`, running `claude` |
| `p` | Popup (throwaway) | 90%x90% popup claude in this cwd; closes when claude exits |
| `c` | Continue here | types `claude --continue` + Enter into the **current** pane |
| `j` | Project picker | popup: zoxide dir list → fzf → jump to that project's existing claude window, else create one |
| `r` | Resume picker | popup: this project's past sessions (mtime + first-prompt excerpt) → fzf → `claude --resume <uuid>` in current pane |
| `P` | Pair-spawn | split + launch claude + wait for readiness + run the mirrored `/pair` handshake automatically |

`Escape`/`q` closes the menu with no side effect. Entries `s/w/p` are pure tmux
one-liners inside the binding; `j/r/P` call scripts in `~/.tmux/scripts/` (new dir,
tracked in this repo — the repo's [.gitignore](../.gitignore) only excludes TPM plugin
dirs, so no ignore change needed).

**Tradeoff, stated up front:** tmux 3.5a (verified `tmux -V`) binds `prefix C` to
`customize-mode -Z` by default. This plan overrides it; customize-mode stays reachable
via `prefix : customize-mode`. If that ever hurts, fallback binding is `prefix M-c`.

### Components

- [tmux.conf](../tmux.conf): the `bind C display-menu` block (below the existing
  `bind P` popup block, above the TPM `run` line — order irrelevant for `bind`, but keeps
  launcher bindings together).
- `scripts/claude-project-dispatch.sh` — zoxide+fzf project jump/create.
- `scripts/claude-resume-picker.sh` — per-project transcript picker.
- `scripts/pair-spawn.sh` — automated pair bootstrap.
- [cheat.txt](../cheat.txt): one new line advertising `prefix C`.

## Implementation sketch

### tmux.conf

```tmux
# prefix + C : Claude launch menu — every way a Claude session starts.
# Overrides the 3.5 default customize-mode (still reachable via `:customize-mode`).
# Scripted entries live in ~/.tmux/scripts/ (tracked in this repo).
bind C display-menu -T "#[align=centre]Claude" -x C -y C \
  "Split right"       s { split-window -h -c "#{pane_current_path}" "claude" } \
  "New window"        w { new-window -c "#{pane_current_path}" -n "cc:#{b:pane_current_path}" "claude" } \
  "Popup (throwaway)" p { display-popup -d "#{pane_current_path}" -w 90% -h 90% -E "claude" } \
  "" \
  "Continue here"     c { send-keys "claude --continue" Enter } \
  "Resume picker"     r { display-popup -w 90% -h 70% -E "~/.tmux/scripts/claude-resume-picker.sh '#{pane_id}' '#{q:pane_current_path}'" } \
  "" \
  "Project picker"    j { display-popup -w 80% -h 60% -E "~/.tmux/scripts/claude-project-dispatch.sh" } \
  "Pair-spawn"        P { run-shell -b "~/.tmux/scripts/pair-spawn.sh '#{pane_id}' '#{q:pane_current_path}'" }
```

### scripts/claude-project-dispatch.sh

```bash
#!/usr/bin/env bash
# Pick a project dir (zoxide frecency order) and jump to its claude window,
# creating one if absent. Runs inside a display-popup (-E: exit closes popup).
set -euo pipefail
dir=$(zoxide query -l | fzf --prompt='claude project> ' --height=100%) || exit 0
# Existing window already sitting in this dir (any session)? Jump to it.
target=$(tmux list-panes -a -F '#{window_id} #{pane_current_path}' \
         | awk -v d="$dir" '$2 == d {print $1; exit}')
if [ -n "${target:-}" ]; then
  tmux switch-client -t "$target" 2>/dev/null || tmux select-window -t "$target"
else
  tmux new-window -c "$dir" -n "cc:$(basename "$dir")" \
    "claude --continue || exec claude"   # no prior session in dir → fresh claude
fi
```

### scripts/claude-resume-picker.sh

```bash
#!/usr/bin/env bash
# fzf over this project's past Claude transcripts; resume by full UUID.
# $1 = invoking pane id (%N), $2 = its cwd. Runs inside a display-popup.
set -euo pipefail
pane=$1; cwd=$2
slug=$(printf '%s' "$cwd" | tr '/.' '--')          # modern slug: /home/fenrir/.tmux → -home-fenrir--tmux
legacy=$(printf '%s' "$cwd" | tr '/' '-')          # older CC versions kept the dot
proj=""
for d in "$HOME/.claude/projects/$slug" "$HOME/.claude/projects/$legacy"; do
  [ -d "$d" ] && proj=$d && break
done
[ -n "$proj" ] || { echo "no past sessions for $cwd"; sleep 1.5; exit 0; }

pick=$(ls -t "$proj"/*.jsonl 2>/dev/null | head -30 | while read -r f; do
  uuid=$(basename "$f" .jsonl)
  when=$(date -r "$f" '+%m-%d %H:%M')
  first=$(jq -r 'select(.type=="user") | .message.content
                 | if type=="string" then . else (map(select(.type=="text"))|.[0].text // empty) end' \
          "$f" 2>/dev/null | grep -m1 -v '^\s*$' | cut -c1-80)
  printf '%s\t%s  %s\n' "$uuid" "$when" "${first:-<no prompt>}"
done | fzf --delimiter='\t' --with-nth=2.. --prompt='resume> ') || exit 0

uuid=${pick%%$'\t'*}
tmux send-keys -t "$pane" "claude --resume $uuid" Enter
```

### scripts/pair-spawn.sh

```bash
#!/usr/bin/env bash
# Split a fresh claude next to the (already-running-claude) caller pane and run
# the mirrored /pair handshake from pair.md: caller listens on p<callerN>,
# newcomer on p<newN>; each is told the OTHER's channel first (A), own second (B).
set -euo pipefail
caller=$1; cwd=$2
new=$(tmux split-window -h -c "$cwd" -P -F '#{pane_id}' "claude")
a="p${caller#%}"; b="p${new#%}"              # pane-derived topic names, e.g. p5 / p7

# Wait for the newcomer's input prompt (poll capture-pane, 30 s cap).
for _ in $(seq 60); do
  tmux capture-pane -p -t "$new" | grep -q '? for shortcuts' && break   # [未驗證] exact idle marker
  sleep 0.5
done

send() {  # literal text, then Enter as a separate keystroke (same trick talk uses)
  tmux send-keys -t "$1" -l "$2"; sleep 0.3; tmux send-keys -t "$1" Enter
}
send "$new"    "/pair $a $b"      # newcomer: other=caller's channel, mine=$b
sleep 2                           # let its mqpub/reader start before the mirror
send "$caller" "/pair $b $a"      # caller: other=newcomer's channel, mine=$a
```

## Integration with existing setup

- **[tmux.conf](../tmux.conf)**: builds on the exact popup idiom already there
  (`bind P display-popup -d "#{pane_current_path}" -w 90% -h 90% -E`) — entry `p` is that
  binding with `claude` as the command. No plugin ordering constraints; `bind` lines can
  sit anywhere before/after TPM.
- **[cheat.txt](../cheat.txt)**: add under OTHER:
  `prefix C        Claude menu: split / window / popup / continue / resume / pair`.
  This is the discoverability channel the sheet's footer mandates instead of new prefixes.
- **`/pair` + mq stack**: `pair-spawn.sh` automates the 6-step bootstrap of
  [pair.md](../../.claude/commands/pair.md) (`mqpub start A B` → `/mqread B`, mirrored).
  Topic names `p<N>` reuse the pane-id convention the talk banner already uses
  (`msg from %N`). The mq binaries (`mqpub`/`mqsend`/`mqreader`, in `~/.local/bin/`,
  verified present) are untouched.
- **talk / hooks**: not invoked. Injection is raw `send-keys -l` + separate Enter — the
  same mechanism talk uses [未驗證], but without talk's PEER-AGENT banner, because `/pair`
  must arrive as a slash command, not as a message. `talk-wrap.sh`, `cross-pane-*` hooks
  fire only on `talk` usage, so no interference.
- **ace-window / thumbs**: no key conflicts — `C` is free of both (`o`/`O` and
  `prefix Space` respectively). Pair-spawn's new pane gets an ace label automatically.
- **Repo mechanics**: `~/.tmux` is a submodule — commit here first, then bless the
  gitlink in the `$HOME` parent. Note `cheat.txt` is currently untracked and `tmux.conf`
  modified in this repo; the MVP commits below fold them in.

## Risks & open questions

- **"Continue here" types into whatever runs in the pane.** If claude (or vim) is already
  focused there, the text lands inside it. Acceptable for v1 (menu is deliberate);
  a guard (`#{pane_current_command}` == shell, else grey the entry with the `-`-prefix
  trick used by tmux's own `prefix <` menu) is a v2 refinement.
- **Idle-prompt marker for pair-spawn is [未驗證]** — `? for shortcuts` must be checked
  against a live claude pane (`tmux capture-pane -p`) on this machine; the string may
  differ by version/state. If polling proves brittle, fall back to a fixed `sleep 8`.
- **Slash-command injection timing**: Claude's slash-command autocomplete popup may
  swallow the Enter if it fires mid-menu. The `sleep 0.3` between text and Enter mirrors
  talk's approach [未驗證 that talk uses exactly this]; verify against talk's source at
  `~/.local/bin/talk` before shipping, and copy its exact delays.
- **jq cost in the resume picker**: one jq pass per transcript; capped at 30 newest.
  Large transcripts stream fine (jq is line-oriented over jsonl), but measure on
  `-home-fenrir` (biggest project dir) before raising the cap.
- **zoxide list contains non-project dirs** (e.g. `/tmp` visits). Harmless — picking one
  just opens claude there — but a `--exclude` filter or intersecting with dirs containing
  `.git` is a possible refinement.
- **Session-vs-window jump**: `switch-client` handles the cross-session case; verify the
  in-popup client can switch (popups run on the attached client, expected to work; [未驗證]).

## MVP steps

Each step is independently testable; commit granularity follows "small system-scoped
commits" (config + its script together).

1. **Menu with the three one-liners + continue** (`s/w/p/c` only) in
   [tmux.conf](../tmux.conf). Test: `tmux source ~/.tmux/tmux.conf`, `prefix C`, run each
   entry, confirm cwd inheritance with `pwd` before typing anything to claude.
2. **`claude-resume-picker.sh`** + menu entry `r`. Test standalone first:
   `~/.tmux/scripts/claude-resume-picker.sh "$(tmux display -p '#{pane_id}')" "$PWD"`
   from `~/.tmux` (has sessions under `-home-fenrir--tmux`, verified) and from a dir with
   none (should print the no-sessions notice and exit cleanly).
3. **`claude-project-dispatch.sh`** + menu entry `j`. Test: pick a dir with an existing
   claude window (jumps), then a fresh dir (creates window named `cc:<base>`; claude
   `--continue` falls back to plain claude when the dir has no history).
4. **`pair-spawn.sh`** + menu entry `P`. Before wiring: manually verify the idle marker
   string and the send-keys delays against a live pane (risks above). Test end-to-end:
   from a claude pane, `prefix C P`, then `mq-status`/`talk read` to confirm both
   readers are up and a message round-trips.
5. **cheat.txt line + docs**: add the OTHER-section line; commit everything in this repo
   (including the pre-existing `tmux.conf` mouse edit and untracked `cheat.txt` if not yet
   committed), then bless the submodule SHA in the `$HOME` parent repo. (Per standing
   rule: commits only, no push.)

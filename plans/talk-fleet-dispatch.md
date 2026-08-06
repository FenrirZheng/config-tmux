# talk fleet dispatch: @role addressing + bcast/collect fan-out

## Problem

Two frictions, both in the dispatch direction (the visibility plans don't touch these):

1. **Addressing by `%N` requires a discovery round-trip.** Every multi-pane session starts
   with "which one is the reviewer — %42 or %43?" — a `talk list`, a squint at pane titles,
   and a mis-send risk when panes get recreated and ids shift. Pane ids are immutable per
   pane but meaningless to a human; the *role* ("reviewer", "impl", "tester") is what the
   supervisor actually thinks in.
2. **Fan-out is hand-assembled N times.** The canonical single-worker pattern in
   [the talk skill](../../.claude/skills/talk/SKILL.md) (marker → `talk send` → `talk ping`
   poll → `talk read-since`) is proven, but fanning one task to 3 workers means hand-writing
   3 markers, 3 poll loops, 3 read-since calls in the supervisor's context. Real failure
   modes observed: marker collisions (same `$RANDOM` reused), premature reads (pane still
   idle right after send, so `read-since` harvests the echoed prompt instead of the answer),
   and the supervisor burning turns babysitting polls instead of doing its own work.

## Design

### User-visible behavior

```bash
talk role set reviewer          # binds "reviewer" to $TMUX_PANE (run in that pane)
talk role set impl %43          # or bind explicitly from anywhere
talk role list                  # reviewer -> %42 (alive)   impl -> %43 (alive)
talk role unset reviewer
talk send @reviewer "check the diff"       # everywhere a target goes, @name now works
talk ping @impl
talk bcast "summarize your open TODOs" @reviewer @impl @tester
# -> prints: round=R1754460000-1234  manifest=/tmp/claude-talk/rounds/R.../manifest.tsv
talk collect R1754460000-1234 --timeout 900
# -> blocks until every worker went busy->idle; writes rounds/<round>/<role-or-pane>.txt
```

A pane with a role shows `[reviewer]` centred in its top border (pane-border-format on
`@claude_role`), so the human sees the fleet layout at a glance. **Pane titles are
deliberately untouched** — `talk ping` reads `#{pane_title}` for idle/busy and must keep
owning it.

The `/bcast` slash command wraps the pair for Claude: it sends via `talk bcast`, launches
`talk collect` with `run_in_background: true`, and the Bash tool re-invokes the supervisor
**exactly once** when collect exits — at which point every answer is already on disk and the
supervisor pays context only for the per-worker result files it chooses to `Read`.

### Components

| piece | where | what |
|---|---|---|
| role registry | `/tmp/claude-talk/roles/<name>` (file body = `%N`) | set/unset/list + GC against `tmux list-panes -a`; resolution errors on dead panes instead of mis-sending |
| `resolve_target()` | inside `talk` | any `<target>` argument starting with `@` resolves through the registry; `%N` / `sess:w.p` pass through unchanged |
| border badge | tmux pane option `@claude_role` + window `pane-border-format` | set at `role set` time (runtime, so it wins over the theme's `pane-border-status off`) |
| `talk bcast` | inside `talk` | per-pane markers `CHK-<round>-<paneN>`, one `send_text` per worker, manifest to `/tmp/claude-talk/rounds/<round>/` |
| `talk collect` | inside `talk` | busy→idle edge detection per pane, then `read-since` marker → `rounds/<round>/<name>.txt`; exits 0 when all harvested |
| `/bcast` command | `~/.claude/commands/bcast.md` | bcast + background collect + read-results-on-wake protocol |

## Implementation sketch

All talk changes go into the source at `~/code/ai-skills/talk-to-ai/talk`
(`~/.local/bin/talk` is a symlink to it — per [the talk skill](../../.claude/skills/talk/SKILL.md)).

```bash
ROLES_DIR=/tmp/claude-talk/roles
ROUNDS_DIR=/tmp/claude-talk/rounds

pane_alive() { tmux list-panes -a -F '#{pane_id}' | grep -qx "$1"; }

resolve_target() {              # @name -> %N ; anything else passes through
  local t=$1
  [[ $t == @* ]] || { printf '%s' "$t"; return 0; }
  local f="$ROLES_DIR/${t#@}" id
  [[ -f $f ]] || die "no such role: $t (talk role list)"
  id=$(<"$f")
  pane_alive "$id" || { rm -f "$f"; die "role $t bound to dead pane $id (unbound; re-run talk role set)"; }
  printf '%s' "$id"
}

cmd_role() {
  require_tmux; mkdir -p "$ROLES_DIR"
  case "${1:-list}" in
    set)   local name=$2 id=${3:-${TMUX_PANE:?}}
           pane_alive "$id" || die "no such pane: $id"
           printf '%s' "$id" > "$ROLES_DIR/$name"
           tmux set-option -p -t "$id" @claude_role "$name"
           tmux set-option -w -t "$id" pane-border-status top
           tmux set-option -w -t "$id" pane-border-format \
             '#[align=centre]#{?@claude_role,#[bold] [#{@claude_role}] #[default],}' ;;
    unset) local id; id=$(cat "$ROLES_DIR/$2" 2>/dev/null || true)
           rm -f "$ROLES_DIR/$2"
           [[ -n $id ]] && pane_alive "$id" && tmux set-option -p -t "$id" -u @claude_role ;;
    list)  for f in "$ROLES_DIR"/*; do [[ -e $f ]] || continue
             local id; id=$(<"$f")
             if pane_alive "$id"; then printf '%-12s %s alive\n' "${f##*/}" "$id"
             else printf '%-12s %s DEAD (gc)\n' "${f##*/}" "$id"; rm -f "$f"; fi
           done ;;
  esac
}
```

`cmd_send` / `cmd_type` / `cmd_read` / `cmd_read_since` / `cmd_ping` each gain one line:
`target=$(resolve_target "$target")`.

```bash
cmd_bcast() {                    # talk bcast <message> <target...>
  require_tmux; local msg=$1; shift
  local round="R$(date +%s)-$$" rdir="$ROUNDS_DIR/$round"; mkdir -p "$rdir"
  local t id marker
  for t in "$@"; do
    id=$(resolve_target "$t")
    marker="CHK-$round-${id#%}"                     # unique per round AND per pane
    printf '%s\t%s\t%s\tpending\n' "$id" "${t#@}" "$marker" >> "$rdir/manifest.tsv"
    send_text "$id" "$msg
[via /talk bcast round=$round] Begin your reply with the exact line:
=== $marker ===" 1
  done
  printf 'round=%s\nmanifest=%s/manifest.tsv\n' "$round" "$rdir"
}

cmd_collect() {                  # talk collect <round> [--timeout N]
  require_tmux
  local round=$1 timeout=${3:-900} rdir="$ROUNDS_DIR/$round" t0=$SECONDS
  [[ -f $rdir/manifest.tsv ]] || die "no such round: $round"
  declare -A seen_busy done
  sleep 5                                            # settle: let workers pick the task up
  while :; do
    local pending=0 id name marker state
    while IFS=$'\t' read -r id name marker _; do
      [[ ${done[$id]:-} ]] && continue
      if ! pane_alive "$id"; then echo "PANE DIED before reply" > "$rdir/$name.txt"; done[$id]=1; continue; fi
      if cmd_ping "$id" >/dev/null 2>&1; then state=idle; else state=busy; fi
      if [[ $state == busy ]]; then seen_busy[$id]=1; pending=$((pending+1))
      elif [[ ${seen_busy[$id]:-} ]]; then           # busy->idle edge: answer is complete
        cmd_read_since "$id" "=== $marker ===" > "$rdir/$name.txt" 2>>"$rdir/errors.log" \
          || echo "MARKER NOT FOUND" > "$rdir/$name.txt"
        done[$id]=1
      else pending=$((pending+1)); fi                # still idle pre-start; keep waiting
    done < "$rdir/manifest.tsv"
    (( pending == 0 )) && { echo "collected -> $rdir"; return 0; }
    (( SECONDS - t0 > timeout )) && { echo "TIMEOUT, partial -> $rdir" >&2; return 1; }
    sleep 3
  done
}
```

`/bcast` command skeleton (`~/.claude/commands/bcast.md`, lives in the `.claude` submodule):

```markdown
---
description: Fan one task out to N role-addressed panes and harvest all replies to files.
argument-hint: <@role...> <task message>
---
1. Split $ARGUMENTS into leading @role tokens and the message body.
2. Run `talk bcast "<body>" <roles...>` (quoted-heredoc form); capture `round=`.
3. Run `talk collect <round> --timeout 900` with run_in_background: true, then
   continue your own work — do NOT poll.
4. When the background task completes, `ls /tmp/claude-talk/rounds/<round>/` and
   Read only the .txt files you need. Report per-role one-liners to the user.
```

## Integration with existing setup

- **talk CLI** ([source symlinked to ~/.local/bin/talk](../../code/ai-skills/talk-to-ai/talk)):
  all new subcommands reuse the existing `send_text`, `cmd_ping`, `cmd_read_since`
  primitives verbatim — no second send path, no new idle heuristic.
- **[talk skill](../../.claude/skills/talk/SKILL.md)**: gains a "@role targets" and
  "bcast/collect" section; its canonical single-worker dispatch pattern stays valid.
- **[talk-wrap.sh hook](../../.claude/hooks/talk-wrap.sh)**: its regex matches only
  `talk (send|with|to)` — `talk bcast` bypasses it. Handled by having bcast embed its own
  `[via /talk bcast round=…]` line (see sketch); the hook's `[via /talk` idempotency check
  then also keeps it from double-wrapping any inner resend. `@name` targets already pass the
  hook's `[^[:space:]]+` target capture unchanged.
- **[/communicate-with](../../.claude/commands/communicate-with.md)**: Step 2 target parsing
  adds the token form `@[a-z0-9_-]+` alongside `%N` / `sess:w.p`; the composed
  `talk send @name …` resolves inside talk.
- **[tmux-ace-window](../plugins/tmux-ace-window/scripts/ace-window.sh)**: it saves and
  restores `pane-border-status` / `pane-border-format` around a jump (`@ace_saved_pbs` /
  `@ace_saved_pbf`, lines 54–55/76–77), so the role badge survives `prefix+o`/`O` unchanged.
- **tokyo-night theme**: [tokyo-night.tmux](../plugins/tokyo-night-tmux/tokyo-night.tmux)
  line 27 forces `pane-border-status off -g`. Because `role set` flips the *window* option at
  runtime (long after theme load), no [tmux.conf](../tmux.conf) ordering hack is needed, and
  windows without roles keep the theme's clean borders.
- **tmux-thumbs**: round result paths like `/tmp/claude-talk/rounds/R…/reviewer.txt` are
  plain paths — already hint-copyable via `prefix+Space`, no regex change needed.
- **[cheat.txt](../cheat.txt)**: add one line under OTHER:
  `talk role set <name>   label this pane; @name works in talk/bcast`.
- **mq**: untouched. bcast/collect is keystroke-injection (tasks *into* interactive panes);
  mq remains the channel for peer messages *between* running sessions. The `/bcast` command
  should say so to prevent channel confusion.
- **beacon plan**: the sketch mentions rendering the role via "the beacon's
  pane-border-format" — no beacon plan exists in `~/.tmux/plans/` yet [未驗證]. This plan is
  therefore self-contained on border rendering; if a beacon feature lands later, merge both
  into one `pane-border-format` string (tmux has only one per window).

## Risks & open questions

- **Idle-detection heuristic inherited from `talk ping`**: title-prefix matching (`✳ ` vs
  braille spinner) breaks if a pane title is renamed or a future Claude Code build changes
  the spinner. collect's busy→idle edge makes this worse than one-shot ping: a worker that
  *never* shows busy (task rejected, permission prompt) waits until timeout. The `sleep 5`
  settle + timeout fallback bounds the damage; a `--no-busy-check` flag is the escape hatch.
- **Marker in scrollback vs. reply**: the marker string appears twice (echoed prompt +
  reply). `cmd_read_since` already takes the *last* occurrence defensively, and the
  busy→idle gate ensures the reply occurrence exists before reading — but a worker that
  ignores the "begin with the marker" instruction yields `MARKER NOT FOUND`; the raw
  fallback is `talk read <id>` by hand.
- **100k scrollback vs. `read_since`'s `-S -5000` window**: very chatty tasks can push the
  marker out of the 5000-line capture. Open question: bump to `-S -20000` for collect only?
- **Role namespace is global across tmux sessions** (one `/tmp/claude-talk/roles/`): two
  sessions both wanting a "reviewer" collide. Acceptable for a single-operator machine;
  if it bites, namespace as `roles/<session>/<name>` and make `@name` resolve session-local
  first.
- **`/tmp` lifetime**: roles and rounds vanish on reboot — correct, since panes do too.
  Stale round dirs accumulate within a boot; `talk collect --gc` or a `find -mtime` sweep is
  a nice-to-have, not MVP.
- **Concurrent rounds to the same worker** would interleave prompts in one Claude input
  box. MVP does not lock; document "one active round per worker" in the skill.

## MVP steps

1. **`talk role set|unset|list` + `resolve_target()`** in the talk source; wire resolution
   into send/type/read/read-since/ping. Test: bind `@a` to a scratch pane, `talk send @a hi`,
   kill the pane, confirm `talk send @a hi` errors *and* unbinds.
2. **Border badge**: the two `set-option` lines in `role set`/`unset`. Test: badge appears
   on set, survives `prefix+o` jump, disappears on unset; role-less windows keep borders off.
3. **`talk bcast`**: manifest + per-pane markers over `send_text`. Test: bcast to 2 scratch
   `bash` panes running `cat`, verify each received its own marker line and
   `manifest.tsv` has 2 pending rows.
4. **`talk collect`**: busy→idle harvest loop. Test: bcast a 30-second task to 2 real Claude
   panes, run collect in a third shell, confirm both `.txt` files contain only post-marker
   content and collect exits 0; repeat with one worker killed mid-task → `PANE DIED` file +
   still exits.
5. **`/bcast` command** in `~/.claude/commands/` (commit inside the `.claude` submodule,
   then bless the gitlink). Test end-to-end from a supervisor Claude: dispatch, keep
   working, get woken exactly once, Read results.
6. **Docs pass**: talk skill sections, `/communicate-with` @name parsing, cheat.txt line.
   Each is an independent one-file edit.

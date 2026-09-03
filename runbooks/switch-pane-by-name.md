# Runbook: switch to a pane by typing its name

How to reach a pane by typing part of its **title** instead of its position.
Shipped as `prefix g` since 2026-09-03 ([`tmux.conf`](../tmux.conf), Option A
below); this runbook records what else exists, what tmux can and cannot do, and
how the key was built. Measured against tmux 3.5a (`tmux -V`) on 2026-09-03.

Related: [`seek`](seek.md), [`sift`](sift.md) search *inside* a pane; this is
about choosing *which* pane.

## The short answer

**tmux cannot address a pane by name.** A pane target accepts an id, an index,
or a positional token, and nothing else — so `select-pane -t 'my-title'` always
fails. Name-typing has to be built as a *filter over a list*, not as a target.

Three things do that here. The first two are not general:

| what | key | scope | matches on |
|---|---|---|---|
| [cc-fleet](../tools/cc-fleet/src/main.rs) | `prefix S` | **Claude panes only** | `@claude_task`, else `#{pane_title}` |
| [ace-window](../plugins/tmux-ace-window) | `prefix o` / `O` | current window only | nothing — positional letters |
| filtered `choose-tree` ([`tmux.conf`](../tmux.conf)) | `prefix g` | **every pane, every session** | `#{pane_title}`, case-insensitive |

If the pane you want is a Claude pane, **`prefix S` is already the answer** and
the rest of this runbook is not needed. fzf filters as you type over a column
that carries the task name ([`cc-fleet/src/main.rs:113`](../tools/cc-fleet/src/main.rs)
— `let label = if p.task.is_empty() { &p.title } else { &p.task };`), and the
column is searchable because fzf runs with `--with-nth=2..`, hiding only the
pane-id field ([`cc-fleet/src/main.rs:247-250`](../tools/cc-fleet/src/main.rs)).
Its roster is `list_claude_panes()`, i.e. panes that pass `is_claude()`
([`tmuxlib/src/lib.rs:291-296`](../tools/tmuxlib/src/lib.rs)) — a shell, an
editor or a log tailer is **not** in that list.

The gap this runbook fills: **a non-Claude pane cannot be reached by name at
all** by either of those. `prefix g` is what closes it.

## What already exists

Before `prefix g`, nothing in this repo selected a pane by matching title
text. Confirmed by grepping the whole tree for `pane_title` — every other hit is
a *reader* or a border format, none is a picker.

- **`prefix o` / `prefix O`** — [ace-window](../plugins/tmux-ace-window),
  loaded at [`tmux.conf:99`](../tmux.conf). Overlays a letter on each pane; you
  press the letter. Positional, current window only. The plugin contains **no
  reference to `pane_title` or to any name** (grepped: `title` appears only in
  `helpers.sh:4`, in the phrase "option-name").
- **`prefix S`** — [cc-fleet](../tools/cc-fleet/src/main.rs), bound at
  [`claude.conf:89`](../claude.conf). The type-to-filter picker described above.
- **`prefix N` / `prefix B`** — [cc-attend](../tools/cc-attend/src/main.rs),
  bound at [`claude.conf:83-84`](../claude.conf). Jumps to the pane that most
  needs attention, ranked; you do not name it.
- **`prefix w`** — tmux's own `choose-tree` ([`cheat.txt:11`](../cheat.txt)).
  Interactive, unfiltered.
- **talk-fleet roles** — `talk-fleet role set <name> [<pane>]` binds a
  human-readable name to a pane id under `/tmp/claude-talk/roles/<name>`, and
  `talk-fleet resolve @<name>` prints the pane id
  ([`talk-fleet/src/main.rs:80-84`](../tools/talk-fleet/src/main.rs) lists the
  four subcommands: `role`, `resolve`, `bcast`, `collect`). **There is no
  `goto` / `switch` subcommand** — roles exist for addressing messages, not for
  moving the client. `talk-fleet resolve` does emit a pane id, so it composes
  with a jump; nothing binds that today. [未驗證] — the role commands were read,
  not run.

### Do not write pane titles

[`tools/ARCHITECTURE.org:35`](../tools/ARCHITECTURE.org) states the constraint:

> *Pane titles are never written.* The layout-resurrect plan wanted
> `select-pane -T <slug>`, but `talk ping` reads `#{pane_title}` to decide
> busy/idle — writing it would break inter-agent messaging for every plan at
> once. The slug lives in `@claude_task` and renders on the border instead.

Both readers are real: `cmd_ping` reads `#{pane_title}`
([`~/.claude/skills/talk/talk:99`](../../.claude/skills/talk/talk)), and
`title_state()` keys off the same two shapes
([`tmuxlib/src/lib.rs:306-316`](../tools/tmuxlib/src/lib.rs)) — a leading `✳`
or the literal `Claude Code` means idle, a leading braille character
(U+2800–U+28FF, the spinner) means busy.

**So `select-pane -T` is off the table for naming Claude panes.** It would
silently reclassify the pane as `Unknown` and drop it out of `prefix S`,
`prefix N`, and `talk ping`.

**Reading titles is free**, and it is what makes the recipe below work: Claude
Code writes the title itself, so those panes are *already named*. Live server,
2026-09-03:

```
%7  | ✳ Switch tmux pane by name  | claude | cc:.tmux
%9  | ✳ OCG 2026 work campion deck | claude | cc:ocg-analysis
%14 | ✳ agents/lead.md 修改        | claude | cc:.claude
```

For a **non-Claude** pane there is no such writer, so you must set the title
yourself with `select-pane -T <name>`. That is safe precisely because the pane
is not a Claude pane — `is_claude()` already returns false for it, and no
reader is watching. Do not do it to a pane running `claude`.

## What tmux gives you natively

### A pane cannot be a name target

`man tmux`, TARGETS ("COMMANDS" section). Windows get name matching:

> 4. An exact window name, such as 'mysession:mywindow'.
> 5. The start of a window name, such as 'mysession:mywin'.
> 6. As an fnmatch(3) pattern matched against the window name.

Panes get none of it:

> target-pane (or src-pane or dst-pane) may be a pane ID or takes a similar
> form to target-window but with the optional addition of a period followed by a
> pane index or pane ID, for example: 'mysession:mywindow.1'.  If the pane index
> is omitted, the currently active pane in the specified window is used.

The token list that follows is entirely positional (`{last}`, `{next}`,
`{top}`, `{left-of}`, …) plus `{mouse}` and `{marked}`. **No title rule
exists.** Verified — with three panes titled `alpha-build`, `beta-server`,
`gamma-notes`:

```
$ tmux select-pane -t 'beta-server'
can't find pane: beta-server        # exit 1
```

### The commands that do work on names

| command | matches | selects |
|---|---|---|
| `find-window [-CNT] pattern` | window **name**, window **title**, visible content | a **window**, not a pane |
| `choose-tree -f <filter>` | any format you write, incl. `#{pane_title}` | session, window **or pane** |
| `display-panes` | nothing — digits `0`-`9` | a pane, positionally |

`find-window` is the near miss: it is `prefix f` by default and it does match
titles, but per `man tmux` it searches "window names, titles, and visible
content" and lands you on a **window**. `#{window_title}` is not
`#{pane_title}` — with pane titles set via `select-pane -T`, `window_title`
measured **empty**, so `find-window -T` would not see them.

`choose-tree -f` is the one that reaches panes. Per `man tmux`:

> -f specifies an initial filter: the filter is a format - if it evaluates to
> zero, the item in the list is not shown, otherwise it is shown.  **If a filter
> would lead to an empty list, it is ignored.**

That last sentence is a trap; see Troubleshoot.

## The recipe

### Option A — filtered tree (interactive, no helper script) — SHIPPED

Bound in [`tmux.conf`](../tmux.conf), next to the ace-window block — it is plain
tmux with no binary behind it, so it does not belong in `claude.conf` with the
tool keys. After an edit, `tmux source-file ~/.tmux.conf`:

```tmux
bind -T prefix g command-prompt -p "pane name:" \
  "choose-tree -Z -f '#{mi:*%%*,#{pane_title}}'"
```

`#{mi:}` not `#{m:}` — `mi` is the case-insensitive form. `%%` is
`command-prompt`'s substitution of what you typed. `prefix g` was **free**
before this binding took it (checked with `tmux list-keys -T prefix`; still free
are `G`, `F`, `j`, `J`).

Type `prefix g`, a fragment of the title, `Enter`. The tree opens showing only
matching panes plus their parent session and window rows.

**Then press `Down` twice, or the pane row's bracketed shortcut key, before
`Enter`.** The cursor starts on the *session* row whenever the filter excludes
the pane you are currently in, and a blind `Enter` on that row runs
`switch-client -t <session>`, which does not move you. This is the single most
likely way to conclude the recipe "does not work".

### Option B — resolve and jump (no list, one keystroke)

The resolution primitive is `list-panes -f`, which takes the same filter format:

```bash
tmux list-panes -a -f '#{mi:*beta*,#{pane_title}}' -F '#{pane_id}' | head -1
```

Feed that to a jump. The repo's own jump idiom is `focus_pane()`
([`tmuxlib/src/lib.rs`](../tools/tmuxlib/src/lib.rs)), which does
`switch-client -t <session>`, `select-window -t <window_id>`, `select-pane -t
<pane>` — three commands, because it targets the window explicitly. A single
`switch-client -t <pane-id>` also works; `man tmux` documents it and it
measured correct:

> As a special case, -t may refer to a pane (a target that contains ':', '.' or
> '%'), to change session, window and pane.

Binding this needs a helper script rather than an inline `run-shell`: the
filter contains `#{...}`, `%%`, single quotes and a command substitution, and
nesting those inside a tmux string is where it breaks. **Guard the empty
case** — `switch-client -t ""` exits **0** and does nothing, so an unguarded
version reports success on a typo. Fail closed: if the id is empty, print a
message and exit non-zero.

### Naming a non-Claude pane

```bash
tmux select-pane -t %12 -T 'db-tail'
```

Persists for the life of the pane unless the program inside sets its own title.
Never do this to a pane running `claude` — see "Do not write pane titles".

## Verify

Everything below was run on a throwaway server (`tmux -L pbnprobe -f /dev/null`)
with three panes titled `alpha-build`, `beta-server`, `gamma-notes`, plus a
second throwaway server holding the attached client that `choose-tree`
requires. The user's real server was only ever read from.

```bash
# the negative control: panes have no name resolution
tmux select-pane -t 'beta-server'          # -> "can't find pane", exit 1

# the filter format
tmux list-panes -a -f '#{mi:*BETA*,#{pane_title}}' -F '#{pane_id} #{pane_title}'
# -> %1 beta-server        (case-insensitive, one row)

# the jump
tmux switch-client -t %0                   # -> exit 0, active pane becomes %0
```

Results, all observed:

| test | result |
|---|---|
| `select-pane -t 'beta-server'` | `can't find pane: beta-server`, exit 1 |
| `#{m:*beta*,#{pane_title}}` per pane | `no` / `MATCH` / `no` |
| `list-panes -a -f` with that filter | one row, `%1 beta-server` |
| `#{mi:}` against `*BETA*` | matched — case-insensitive confirmed |
| `choose-tree -Z -f '#{mi:*beta*,…}'` | tree showed only `1: bash: "beta-server"` |
| `Down Down Enter` in that tree | active pane became `%1 beta-server` |
| shortcut key `2` in that tree | active pane became `%0 alpha-build` |
| **Enter with no navigation** | **active pane unchanged** — cursor was on the session row |
| full `prefix g` → `gamma` → `Enter` → `Down Down Enter` | active pane became `%2 gamma-notes` |
| `switch-client -t %0` | exit 0, pane changed |
| `switch-client -t ""` | exit 0, **nothing happened** |
| filter matching nothing | filter ignored, **all three panes shown** |
| `window_title` with pane titles set | empty string |

Not verified [未驗證]:

- The bound key exercised interactively on the **real** server. The line is now
  installed in [`tmux.conf`](../tmux.conf) and `tmux list-keys -T prefix g`
  prints it after `source-file` (checked 2026-09-03), but a keypress cannot be
  delivered from a script — see the testing note above. The behaviour was
  exercised verbatim on the throwaway server.
- `talk-fleet role set` / `resolve` end to end — source read, not run.
- Option B as a bound key. Its two halves were each measured; the helper script
  that joins them was not written.

Testing note: `tmux send-keys -t <pane> C-b g` does **not** trigger a prefix
binding — `send-keys` injects into the pane and bypasses the key table. Keys
must be delivered to the terminal of the attached client, which is why the
verification used a nested server and sent keys to the *outer* pane.

## Troubleshoot

**Typing a name shows every pane instead of one** — the filter matched nothing,
and `man tmux` says a filter leading to an empty list "is ignored". A full tree
after typing a name means **zero matches**, not a broken filter. Check the
titles you are matching against:

```bash
tmux list-panes -a -F '#{pane_id} [#{pane_title}]'
```

**The tree filters correctly but `Enter` does not move me** — the cursor is on
the session or window row, not the pane row. Press `Down` until the highlighted
line is the one reading `<index>: <command>: "<title>"`, or press the bracketed
shortcut key at the left of that row. Verified: blind `Enter` left the active
pane unchanged; `Down Down Enter` moved it.

**A Claude pane disappeared from `prefix S` / `prefix N`** — something wrote its
pane title. `is_claude()` falls back to the title shape when no `@claude_state`
is stamped ([`tmuxlib/src/lib.rs:291-296`](../tools/tmuxlib/src/lib.rs)), so a
custom title reclassifies the pane as `Unknown`. Do not `select-pane -T` a
Claude pane. There is no unset; restart the pane's Claude session, or wait for
the next hook to stamp `@claude_state`.

**`talk ping` reports the wrong state** — same cause, same fix.

**The jump silently does nothing** — `switch-client -t ""` exits 0. The
resolution step found no pane and the empty string was passed through. Guard it.

**A name matches two panes** — `list-panes -f | head -1` picks arbitrarily
(source order, not relevance). Use Option A, which shows you both.

**`prefix f` (`find-window`) does not find my pane title** — by design. It
searches window names, window titles and visible content, and selects a window.
`#{window_title}` measured empty while pane titles were set.

## Roll back

Option A ships. To drop it, delete the `bind -T prefix g` line from
[`tmux.conf`](../tmux.conf) and run `tmux source-file ~/.tmux.conf`.

Note that `source-file` does **not** remove a stale binding from a running
server — you must `unbind g` explicitly, the same hazard recorded at
[`tmux.conf:18-23`](../tmux.conf) and in [seek.md](seek.md).

Titles set by hand with `select-pane -T` persist for the life of the pane; kill
the pane or let the program overwrite the title.
